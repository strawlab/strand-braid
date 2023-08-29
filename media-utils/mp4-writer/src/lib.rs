// Copyright 2022-2023 Andrew D. Straw.

//! # mp4-writer
//!
//! MP4 data (or in .h264 files) can be inspected with:
//!     ffprobe -select_streams v -show_packets -show_data <filename.ext>

// Regarding timestamps
//
// This code should ensure that an MP4 file with H264 video data should ideally
// have the creation time in the start metadata
// (`ci2_remote_control::H264Metadata`) equal to the precision time stamp of the
// initial frame. (Although, to specify the timezone, the creation time may be
// in a timezone other than UTC.)

#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access)
)]

use std::rc::Rc;

#[macro_use]
extern crate log;

use ci2_remote_control::{H264Metadata, Mp4RecordingConfig, H264_METADATA_UUID};
use convert_image::convert_into;

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};

use frame_source::h264_annexb_split;
use machine_vision_formats::{
    pixel_format, ImageBuffer, ImageBufferRef, ImageData, ImageStride, PixelFormat, Stride,
};
use nvenc::{InputBuffer, OutputBuffer, RateControlMode};

use thiserror::Error;

// The number of time units that pass in one second.
const MOVIE_TIMESCALE: u32 = 1_000_000;
const TRACK_ID: u32 = 1;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{source}")]
    Mp4Error {
        #[from]
        source: mp4::Error,
    },
    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("required h264 data (SPS or PPS) not found")]
    RequiredH264DataNotFound {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("file already closed")]
    FileAlreadyClosed {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("cannot encode frame when copying h264 stream")]
    RawH264CopyCannotEncodeFrame {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("bad input data")]
    BadInputData {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("inconsistent state")]
    InconsistentState {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("timestamp too large")]
    TimestampTooLarge {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("convert image error")]
    ConvertImageError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        convert_image::Error,
    ),
    #[cfg(feature = "openh264")]
    #[error("openhs264 error {}", inner)]
    OpenH264Error {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        inner: openh264::Error,
    },
    #[error("nvenc error")]
    NvencError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        nvenc::NvEncError,
    ),
    #[error("nvenc libraries not loaded")]
    NvencLibsNotLoaded,
    #[error("less-avc error {}", inner)]
    LessAvcWrapperError {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        inner: less_avc_wrapper::Error,
    },
}

impl From<dynlink_nvidia_encode::NvencError> for Error {
    fn from(orig: dynlink_nvidia_encode::NvencError) -> Self {
        Error::NvencError(orig.into())
    }
}

impl From<dynlink_cuda::CudaError> for Error {
    fn from(orig: dynlink_cuda::CudaError) -> Self {
        Error::NvencError(orig.into())
    }
}

type Result<T> = std::result::Result<T, Error>;

enum MyEncoder<'lib> {
    CopyRawH264 {
        h264_parser: H264Parser,
    },
    Nvidia(NvEncoder<'lib>),
    #[cfg(feature = "openh264")]
    OpenH264(OpenH264Encoder),
    LessH264(LessEncoderWrapper),
}

/// A view of image to have new width
pub struct TrimmedImage<'a, FMT> {
    pub orig: &'a dyn ImageStride<FMT>,
    pub width: u32,
    pub height: u32,
}

impl<'a, FMT> ImageData<FMT> for TrimmedImage<'a, FMT> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn buffer_ref(&self) -> ImageBufferRef<'_, FMT> {
        self.orig.buffer_ref()
    }
    fn buffer(self) -> ImageBuffer<FMT> {
        // copy the buffer
        self.orig.buffer_ref().to_buffer()
    }
}

impl<'a, FMT> Stride for TrimmedImage<'a, FMT> {
    fn stride(&self) -> usize {
        self.orig.stride()
    }
}

/// Trim input image to be divisible by 2 width and height.
pub fn trim_image<FMT>(orig: &dyn ImageStride<FMT>, width: u32, height: u32) -> TrimmedImage<FMT> {
    TrimmedImage {
        orig,
        width: (width / 2) * 2,
        height: (height / 2) * 2,
    }
}

pub struct Mp4Writer<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    inner: Option<WriteState<'lib, T>>,
    nv_enc: Option<nvenc::NvEnc<'lib>>,
}

impl<'lib, T> Mp4Writer<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    pub fn new(
        fd: T,
        config: Mp4RecordingConfig,
        nv_enc: Option<nvenc::NvEnc<'lib>>,
    ) -> Result<Self> {
        let h264_parser = H264Parser::new(config.h264_metadata.clone());
        Ok(Self {
            inner: Some(WriteState::Configured(Box::new((fd, config, h264_parser)))),
            nv_enc,
        })
    }

    /// Low-level writer which saves a buffer which is already h264 encoded.
    ///
    /// This skips the automatic encoding which would normally be done.
    pub fn write_h264_buf(
        &mut self,
        data: &frame_source::H264EncodingVariant,
        width: u32,
        height: u32,
        timestamp: chrono::DateTime<chrono::Utc>,
        frame0_time: chrono::DateTime<chrono::Utc>,
        insert_precision_timestamp: bool,
    ) -> Result<()> {
        let inner = self.inner.take();

        let is_keyframe = parse_h264_is_idr_frame(data)?;

        let pts = timestamp - frame0_time;
        let mp4_sample_start_time = dur2raw(&pts.to_std().unwrap());
        let sample = match &data {
            frame_source::H264EncodingVariant::AnnexB(buf) => {
                let nals = h264_annexb_split(&buf[..]).collect();

                EbspNals {
                    pts,
                    mp4_sample_start_time,
                    is_keyframe,
                    nals,
                }
            }
            frame_source::H264EncodingVariant::Avcc(bufs) => {
                let nal_iter = iter_avcc_bufs(bufs);
                let mut nals = Vec::new();
                for nal_ebsp_bytes in nal_iter {
                    let nal_ebsp_bytes = nal_ebsp_bytes?;
                    nals.push(nal_ebsp_bytes.to_vec());
                }
                EbspNals {
                    pts,
                    mp4_sample_start_time,
                    is_keyframe,
                    nals,
                }
            }
            frame_source::H264EncodingVariant::RawEbsp(nals) => EbspNals {
                pts,
                mp4_sample_start_time,
                is_keyframe,
                nals: nals.clone(),
            },
        };

        let mut state = match inner {
            Some(WriteState::Configured(mut mybox)) => {
                let (fd, _cfg, ref mut h264_parser) = *mybox;
                if insert_precision_timestamp {
                    h264_parser.push_nals(sample, Some(timestamp));
                } else {
                    h264_parser.push_nals(sample, None);
                }
                let mp4_writer = start_mp4_writer(
                    fd,
                    h264_parser
                        .sps
                        .as_ref()
                        .ok_or(Error::RequiredH264DataNotFound {
                            #[cfg(feature = "backtrace")]
                            backtrace: std::backtrace::Backtrace::capture(),
                        })?,
                    h264_parser
                        .pps
                        .as_ref()
                        .ok_or(Error::RequiredH264DataNotFound {
                            #[cfg(feature = "backtrace")]
                            backtrace: std::backtrace::Backtrace::capture(),
                        })?,
                    width,
                    height,
                )?;
                let mp4_segment = MaybeMp4Writer::Mp4Writer(mp4_writer);
                let my_encoder = MyEncoder::CopyRawH264 {
                    h264_parser: h264_parser.clone(),
                };
                Box::new(RecordingState {
                    mp4_segment,
                    my_encoder,
                    inner: None,
                })
            }
            Some(WriteState::Recording(mut state)) => {
                match &mut state.my_encoder {
                    &mut MyEncoder::CopyRawH264 {
                        ref mut h264_parser,
                    } => {
                        if insert_precision_timestamp {
                            h264_parser.push_nals(sample, Some(timestamp));
                        } else {
                            h264_parser.push_nals(sample, None);
                        }
                    }
                    _ => {
                        panic!();
                    }
                }
                state
            }
            None | Some(WriteState::Finished) => {
                return inconsistent_state_err();
            }
        };

        if state.inner.is_some() {
            return inconsistent_state_err();
        }

        let sample = match &mut state.my_encoder {
            &mut MyEncoder::CopyRawH264 {
                ref mut h264_parser,
            } => h264_parser.avcc_sample().unwrap(),
            _ => {
                panic!();
            }
        };

        match &mut state.mp4_segment {
            MaybeMp4Writer::Mp4Writer(mp4_writer) => {
                mp4_writer.write_sample(TRACK_ID, &sample)?;
            }
            _ => {
                return inconsistent_state_err();
            }
        }
        self.inner = Some(WriteState::Recording(state));

        Ok(())
    }

    pub fn write_dynamic(
        &mut self,
        frame: &DynamicFrame,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        match_all_dynamic_fmts!(frame, x, {
            self.write(x, timestamp)?;
        });
        Ok(())
    }

    pub fn write<'a, IM, FMT>(
        &'a mut self,
        frame: &IM,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()>
    where
        IM: ImageStride<FMT>,
        FMT: PixelFormat,
    {
        let inner = self.inner.take();

        match inner {
            Some(WriteState::Configured(mybox)) => {
                let (fd, cfg, h264_parser) = *mybox;
                let frame = trim_image(frame, frame.width(), frame.height());

                let width = frame.width();
                let height = frame.height();

                let mut opt_nv_h264_encoder = None;

                match &cfg.codec {
                    ci2_remote_control::Mp4Codec::H264RawStream => {}
                    ci2_remote_control::Mp4Codec::H264LessAvc => {}
                    ci2_remote_control::Mp4Codec::H264OpenH264(_) => {}
                    ci2_remote_control::Mp4Codec::H264NvEnc(ref opts) => {
                        // scope for anonymous lifetime of ref
                        match &self.nv_enc {
                            Some(ref nv_enc) => {
                                debug!("Using codec H264 in mp4 file.");

                                // Setup the encoder.
                                let cuda_version = nv_enc.cuda_version()?;
                                info!("CUDA version {}", cuda_version);

                                let nvenc_version = nv_enc
                                    .libnvenc
                                    .api_get_max_supported_version()
                                    .map_err(nvenc::NvEncError::from)?;
                                info!(
                                    "NV_ENC version {}.{}",
                                    nvenc_version.major, nvenc_version.minor
                                );

                                // From the Nvidia SDK docs for NvEncCreateInputBuffer: "The number of input
                                // buffers to be allocated by the client must be at least 4 more than the
                                // number of B frames being used for encoding."
                                let num_bufs = 60;

                                let dev = nv_enc.libcuda.new_device(opts.cuda_device)?;

                                info!("CUDA device: {}, name: {}", opts.cuda_device, dev.name()?);
                                let ctx = dev.into_context()?;
                                let encoder: Rc<nvenc::Encoder<'lib>> =
                                    nv_enc.functions.new_encoder(ctx)?;

                                let encode = nvenc::NV_ENC_CODEC_H264_GUID;
                                // let encode = nvenc::NV_ENC_CODEC_HEVC_GUID;
                                let preset = nvenc::NV_ENC_PRESET_HP_GUID;
                                // let preset = nvenc::NV_ENC_PRESET_DEFAULT_GUID;
                                let format = nvenc::BufferFormat::NV12;

                                let param_builder =
                                    nvenc::InitParamsBuilder::new(encode, width, height)
                                        // .ptd(true)
                                        .preset_guid(preset);

                                let param_builder =
                                    match cfg.max_framerate.as_numerator_denominator() {
                                        Some((num, den)) => param_builder.set_framerate(num, den),
                                        None => param_builder,
                                    };

                                let mut encoder_config =
                                    encoder.get_encode_preset_config(encode, preset)?;
                                encoder_config.set_rate_control_mode(RateControlMode::Vbr);
                                encoder_config.set_average_bit_rate(opts.bitrate * 1000);
                                encoder_config.set_max_bit_rate(opts.bitrate * 1000);

                                let params =
                                    param_builder.set_encode_config(encoder_config).build()?;

                                match encoder.initialize(&params) {
                                    Ok(()) => Ok(()),
                                    Err(e) => {
                                        log::error!(
                                            "failed initializing nvenc with params: {:?}",
                                            params
                                        );
                                        Err(e)
                                    }
                                }?;

                                let input_buffers: Vec<InputBuffer<'lib>> = (0..num_bufs)
                                    .map(|_| {
                                        nvenc::Encoder::alloc_input_buffer(
                                            &encoder, width, height, format,
                                        )
                                    })
                                    .collect::<std::result::Result<Vec<_>, _>>()?;

                                let output_buffers: Vec<_> = (0..num_bufs)
                                    .map(|_| nvenc::Encoder::alloc_output_buffer(&encoder))
                                    .collect::<std::result::Result<Vec<_>, _>>()?;

                                let vram_buffers: Vec<IOBuffer<_, _>> = input_buffers
                                    .into_iter()
                                    .zip(output_buffers.into_iter())
                                    .map(|(i, o)| IOBuffer {
                                        in_buf: i,
                                        out_buf: o,
                                    })
                                    .collect();

                                let vram_queue = nvenc::Queue::new(vram_buffers);

                                opt_nv_h264_encoder = Some(NvEncoder {
                                    encoder,
                                    h264_parser: h264_parser.clone(),
                                    // annex_b_reader,
                                    vram_queue,
                                    first_timestamp: timestamp,
                                });
                            }
                            None => return Err(Error::NvencLibsNotLoaded),
                        }
                    }
                };

                let my_encoder = match cfg.codec {
                    ci2_remote_control::Mp4Codec::H264RawStream => MyEncoder::CopyRawH264 {
                        // metadata,
                        h264_parser,
                    },
                    ci2_remote_control::Mp4Codec::H264LessAvc => {
                        MyEncoder::LessH264(LessEncoderWrapper {
                            encoder: Default::default(),
                            h264_parser,
                            first_timestamp: timestamp,
                        })
                    }
                    #[allow(unused_variables)]
                    ci2_remote_control::Mp4Codec::H264OpenH264(opts) => {
                        #[cfg(feature = "openh264")]
                        {
                            let cfg = openh264::encoder::EncoderConfig::new(width, height)
                                .debug(opts.debug())
                                .enable_skip_frame(opts.enable_skip_frame())
                                .rate_control_mode(convert_openh264_rc_mode(
                                    opts.rate_control_mode(),
                                ))
                                .set_bitrate_bps(opts.bitrate_bps());

                            MyEncoder::OpenH264(OpenH264Encoder {
                                encoder: openh264::encoder::Encoder::with_config(cfg)?,
                                h264_parser,
                                first_timestamp: timestamp,
                            })
                        }
                        #[cfg(not(feature = "openh264"))]
                        {
                            // We should never get here.
                            panic!("No Open H264 support at compilation time.");
                        }
                    }
                    ci2_remote_control::Mp4Codec::H264NvEnc(_) => {
                        let enc = opt_nv_h264_encoder.unwrap();
                        MyEncoder::Nvidia(enc)
                    }
                };

                let inner = RecordingStateInner {
                    first_timestamp: timestamp,
                    previous_timestamp: timestamp,
                    interval_for_limiting_fps: chrono::Duration::from_std(
                        cfg.max_framerate.interval(),
                    )
                    .unwrap(),
                    trim_width: width,
                    trim_height: height,
                };

                let mut state = RecordingState {
                    mp4_segment: MaybeMp4Writer::Starting(fd),
                    my_encoder,
                    inner: Some(inner),
                };

                write_frame(&mut state, &frame, timestamp)?;

                self.inner = Some(WriteState::Recording(Box::new(state)));

                Ok(())
            }
            Some(WriteState::Recording(mut state)) => {
                let frame = if let Some(state_inner) = &mut state.inner {
                    let interval = timestamp.signed_duration_since(state_inner.previous_timestamp);
                    if interval >= state_inner.interval_for_limiting_fps {
                        let frame =
                            trim_image(frame, state_inner.trim_width, state_inner.trim_height);
                        debug!("Saving frame at {}: interval {}", timestamp, interval);

                        state_inner.previous_timestamp = timestamp;
                        Some(frame)
                    } else {
                        debug!(
                            "Not saving frame at {}: interval {} too small",
                            timestamp, interval
                        );
                        None
                    }
                } else {
                    return inconsistent_state_err();
                };
                if let Some(frame) = frame {
                    write_frame(&mut state, &frame, timestamp)?;
                }
                self.inner = Some(WriteState::Recording(state));

                Ok(())
            }
            Some(WriteState::Finished) => {
                self.inner = Some(WriteState::Finished);
                Err(Error::FileAlreadyClosed {
                    #[cfg(feature = "backtrace")]
                    backtrace: std::backtrace::Backtrace::capture(),
                })
            }

            None => Err(Error::InconsistentState {
                #[cfg(feature = "backtrace")]
                backtrace: std::backtrace::Backtrace::capture(),
            }),
        }
    }

    /// Finish writing the MP4 file.
    ///
    /// Calling this allows any errors to be caught explicitly. Otherwise,
    /// the MP4 file will be finished when the writer is dropped. In that case,
    /// any errors will result in a panic.
    pub fn finish(&mut self) -> Result<()> {
        let inner = self.inner.take();
        match inner {
            Some(WriteState::Configured(_)) => {
                // no frames written.
                self.inner = Some(WriteState::Finished);
                Ok(())
            }
            Some(WriteState::Recording(mut state)) => {
                match state.my_encoder {
                    MyEncoder::CopyRawH264 { h264_parser: _ } | MyEncoder::LessH264(_) => { /* nothing to do */
                    }
                    #[cfg(feature = "openh264")]
                    MyEncoder::OpenH264(_encoder) => { /* nothing to do */ }
                    MyEncoder::Nvidia(ref mut nv_encoder) => {
                        nv_encoder.encoder.end_stream()?;
                        // Now done with all frames, drain the pending data.
                        loop {
                            let sample = match nv_encoder.vram_queue.get_pending() {
                                None => break,
                                Some(iobuf) => {
                                    // scope for locked output buffer
                                    let outbuf = iobuf.out_buf.lock()?;
                                    nv_outbuf_to_sample(outbuf)
                                }
                            };
                            if let Some(state_inner) = state.inner.as_ref() {
                                nv_encoder.inner_save_data(
                                    &mut state.mp4_segment,
                                    sample,
                                    state_inner.trim_width,
                                    state_inner.trim_height,
                                )?;
                            } else {
                                return Err(Error::InconsistentState {
                                    #[cfg(feature = "backtrace")]
                                    backtrace: std::backtrace::Backtrace::capture(),
                                });
                            }
                        }
                    }
                }

                if let MaybeMp4Writer::Mp4Writer(mut mp4_writer) = state.mp4_segment {
                    mp4_writer.write_end()?;
                }

                trace!("Finalized video.");
                self.inner = Some(WriteState::Finished);
                Ok(())
            }
            Some(WriteState::Finished) => {
                self.inner = Some(WriteState::Finished);
                Err(Error::FileAlreadyClosed {
                    #[cfg(feature = "backtrace")]
                    backtrace: std::backtrace::Backtrace::capture(),
                })
            }
            None => Err(Error::InconsistentState {
                #[cfg(feature = "backtrace")]
                backtrace: std::backtrace::Backtrace::capture(),
            }),
        }
    }
}

fn nv_outbuf_to_sample(outbuf: dynlink_nvidia_encode::api::LockedOutputBuffer) -> EbspNals {
    let nals = h264_annexb_split(outbuf.mem()).collect();

    EbspNals {
        pts: chrono::Duration::from_std(*outbuf.pts()).unwrap(),
        mp4_sample_start_time: outbuf.output_time_stamp(),
        is_keyframe: outbuf.is_keyframe(),
        nals,
    }
}

impl<'lib, T> Drop for Mp4Writer<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    fn drop(&mut self) {
        match &self.inner {
            // Happy path when .finish() already called.
            Some(WriteState::Finished) => {}
            // Error happened in self.write().
            None => {}
            // When .finished() not already called.
            Some(_) => {
                if !std::thread::panicking() {
                    // We are being dropping, so finish the file.
                    self.finish().unwrap()
                } else {
                    // We are being dropped, but we are unwinding, so just leave
                    // the file as-is. (Should we even truncate it?)
                }
            }
        }
    }
}

trait PtsDur {
    fn pts_dur(&self) -> std::time::Duration;
}

fn write_frame<T, FRAME, FMT>(
    state: &mut RecordingState<'_, T>,
    raw_frame: &FRAME,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<()>
where
    T: std::io::Write + std::io::Seek,
    FRAME: ImageStride<FMT>,
    FMT: PixelFormat,
{
    match (&mut state.my_encoder, &state.inner) {
        (MyEncoder::CopyRawH264 { h264_parser: _ }, _) => {
            return Err(Error::RawH264CopyCannotEncodeFrame {
                #[cfg(feature = "backtrace")]
                backtrace: std::backtrace::Backtrace::capture(),
            });
        }
        (MyEncoder::LessH264(encoder), Some(state_inner)) => {
            let nals = encoder.encoder.encode_to_nal_units(raw_frame)?;

            let is_keyframe = true;

            let pts = timestamp - encoder.first_timestamp;
            let mp4_sample_start_time = dur2raw(&pts.to_std().unwrap());

            let sample = EbspNals {
                pts,
                mp4_sample_start_time,
                is_keyframe,
                nals,
            };

            encoder.inner_save_data(
                &mut state.mp4_segment,
                sample,
                state_inner.trim_width,
                state_inner.trim_height,
            )?;
        }
        #[cfg(feature = "openh264")]
        (MyEncoder::OpenH264(encoder), Some(state_inner)) => {
            // todo: bitrate, keyframes, timestamp check and duration finding.

            let y4m = convert_image::encode_y4m_frame(
                raw_frame,
                convert_image::Y4MColorspace::C420paldv,
                None,
            )?;

            let encoded = encoder.encoder.encode(&YUVData::from(y4m)).unwrap();

            use openh264::encoder::FrameType;
            let is_keyframe =
                (encoded.frame_type() == FrameType::IDR) | (encoded.frame_type() == FrameType::I);

            // todo: preallocate and keep buffer available by using write_vec
            let annex_b_data = encoded.to_vec();

            let nals = h264_annexb_split(&annex_b_data).collect();

            let pts = timestamp - encoder.first_timestamp;
            let mp4_sample_start_time = dur2raw(&pts.to_std().unwrap());

            let sample = EbspNals {
                pts,
                mp4_sample_start_time,
                is_keyframe,
                nals,
            };

            encoder.inner_save_data(
                &mut state.mp4_segment,
                sample,
                state_inner.trim_width,
                state_inner.trim_height,
            )?;
        }
        (MyEncoder::Nvidia(ref mut nv_encoder), Some(state_inner)) => {
            let vram_buf: &mut IOBuffer<_, _> = match nv_encoder.vram_queue.get_available() {
                Some(iobuf) => iobuf,
                None => {
                    let sample = {
                        let iobuf = nv_encoder.vram_queue.get_pending().expect("get pending");
                        // scope for locked output buffer
                        let outbuf = iobuf.out_buf.lock()?;
                        nv_outbuf_to_sample(outbuf)
                    };
                    nv_encoder.inner_save_data(
                        &mut state.mp4_segment,
                        sample,
                        state_inner.trim_width,
                        state_inner.trim_height,
                    )?;
                    nv_encoder
                        .vram_queue
                        .get_available()
                        .expect("get available")
                }
            };

            // Now we have an "available" buffer in the encoder.

            let pitch = {
                // Scope for locked input buffer.
                let mut inbuf = vram_buf.in_buf.lock()?;
                let dest_stride = inbuf.pitch();

                let mut dest = image_iter::ReinterpretedImageMut {
                    orig: inbuf.mem_mut(),
                    width: raw_frame.width(),
                    height: raw_frame.height(),
                    stride: dest_stride,
                    fmt: std::marker::PhantomData::<pixel_format::NV12>,
                };

                convert_into(raw_frame, &mut dest)?;
                // Now vram_buf.in_buf has the nv12 encoded data.
                dest_stride
            };

            let elapsed = timestamp.signed_duration_since(state_inner.first_timestamp);
            let pts = elapsed.to_std().unwrap();

            nv_encoder
                .encoder
                .encode_picture(&vram_buf.in_buf, &vram_buf.out_buf, pitch, pts)?;
        }
        (_encoder, None) => {
            return inconsistent_state_err();
        }
    }
    Ok(())
}

enum WriteState<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    Configured(Box<(T, Mp4RecordingConfig, H264Parser)>),
    Recording(Box<RecordingState<'lib, T>>),
    Finished,
}

struct RecordingState<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    mp4_segment: MaybeMp4Writer<T>,
    my_encoder: MyEncoder<'lib>,
    inner: Option<RecordingStateInner>,
}

struct RecordingStateInner {
    first_timestamp: chrono::DateTime<chrono::Utc>,
    previous_timestamp: chrono::DateTime<chrono::Utc>,
    /// limits the maximum framerate
    interval_for_limiting_fps: chrono::Duration,
    trim_width: u32,
    trim_height: u32,
}

struct LessEncoderWrapper {
    encoder: less_avc_wrapper::WrappedLessEncoder,
    h264_parser: H264Parser,
    first_timestamp: chrono::DateTime<chrono::Utc>,
}

impl LessEncoderWrapper {
    fn compute_utc_timestamp(&self, sample: &EbspNals) -> chrono::DateTime<chrono::Utc> {
        self.first_timestamp + sample.pts
    }
    fn inner_save_data<T>(
        &mut self,
        mp4_segment: &mut MaybeMp4Writer<T>,
        sample: EbspNals,
        trim_width: u32,
        trim_height: u32,
    ) -> Result<()>
    where
        T: std::io::Write + std::io::Seek,
    {
        let utc_timestamp = self.compute_utc_timestamp(&sample);
        self.h264_parser.push_nals(sample, Some(utc_timestamp));
        let sps = self.h264_parser.sps().unwrap();
        let pps = self.h264_parser.pps().unwrap();

        let mut mp4_writer = match std::mem::replace(mp4_segment, MaybeMp4Writer::Nothing) {
            MaybeMp4Writer::Mp4Writer(mp4_writer) => mp4_writer,
            MaybeMp4Writer::Starting(fd) => {
                start_mp4_writer(fd, sps, pps, trim_width, trim_height)?
            }
            MaybeMp4Writer::Nothing => {
                panic!("inconsistent state");
            }
        };

        let avcc_sample = self.h264_parser.avcc_sample().unwrap();
        mp4_writer.write_sample(TRACK_ID, &avcc_sample)?;

        *mp4_segment = MaybeMp4Writer::Mp4Writer(mp4_writer);

        Ok(())
    }
}

struct NvEncoder<'lib> {
    encoder: Rc<nvenc::Encoder<'lib>>,
    h264_parser: H264Parser,
    vram_queue: nvenc::Queue<IOBuffer<InputBuffer<'lib>, OutputBuffer<'lib>>>,
    first_timestamp: chrono::DateTime<chrono::Utc>,
}

impl<'lib> NvEncoder<'lib> {
    fn compute_utc_timestamp(&self, sample: &EbspNals) -> chrono::DateTime<chrono::Utc> {
        self.first_timestamp + sample.pts
    }
    fn inner_save_data<T>(
        &mut self,
        mp4_segment: &mut MaybeMp4Writer<T>,
        sample: EbspNals,
        trim_width: u32,
        trim_height: u32,
    ) -> Result<()>
    where
        T: std::io::Write + std::io::Seek,
    {
        let utc_timestamp = self.compute_utc_timestamp(&sample);
        self.h264_parser.push_nals(sample, Some(utc_timestamp));
        let mut mp4_writer = match std::mem::replace(mp4_segment, MaybeMp4Writer::Nothing) {
            MaybeMp4Writer::Mp4Writer(mp4_writer) => mp4_writer,
            MaybeMp4Writer::Starting(fd) => {
                let sps = self.h264_parser.sps().unwrap();
                let pps = self.h264_parser.pps().unwrap();
                start_mp4_writer(fd, sps, pps, trim_width, trim_height)?
            }
            MaybeMp4Writer::Nothing => {
                panic!("inconsistent state");
            }
        };

        let avcc_sample = self.h264_parser.avcc_sample().unwrap();
        mp4_writer.write_sample(TRACK_ID, &avcc_sample)?;

        *mp4_segment = MaybeMp4Writer::Mp4Writer(mp4_writer);

        Ok(())
    }
}

fn start_mp4_writer<T>(
    fd: T,
    sps: &[u8],
    pps: &[u8],
    trim_width: u32,
    trim_height: u32,
) -> Result<mp4::Mp4Writer<T>>
where
    T: std::io::Write + std::io::Seek,
{
    let mp4_config = mp4::Mp4Config {
        major_brand: str::parse("isom").unwrap(),
        minor_version: 512,
        compatible_brands: vec![
            str::parse("isom").unwrap(),
            str::parse("iso2").unwrap(),
            str::parse("avc1").unwrap(),
            str::parse("mp41").unwrap(),
        ],
        // This is `movie_timescale`, the number of
        // time units that pass in one second.
        timescale: MOVIE_TIMESCALE,
    };

    let mut mp4_writer = mp4::Mp4Writer::write_start(fd, &mp4_config)?;

    let media_conf = mp4::MediaConfig::AvcConfig(mp4::AvcConfig {
        width: trim_width.try_into().unwrap(),
        height: trim_height.try_into().unwrap(),
        seq_param_set: sps.to_vec(),
        pic_param_set: pps.to_vec(),
    });

    let track_conf = mp4::TrackConfig {
        track_type: mp4::TrackType::Video,
        timescale: MOVIE_TIMESCALE,
        language: String::from("eng"),
        media_conf,
    };

    mp4_writer.add_track(&track_conf)?;
    Ok(mp4_writer)
}

#[cfg(feature = "openh264")]
struct OpenH264Encoder {
    encoder: openh264::encoder::Encoder,
    h264_parser: H264Parser,
    first_timestamp: chrono::DateTime<chrono::Utc>,
}

#[cfg(feature = "openh264")]
impl OpenH264Encoder {
    fn compute_utc_timestamp(&self, sample: &EbspNals) -> chrono::DateTime<chrono::Utc> {
        self.first_timestamp + sample.pts
    }
    fn inner_save_data<T>(
        &mut self,
        mp4_segment: &mut MaybeMp4Writer<T>,
        sample: EbspNals,
        trim_width: u32,
        trim_height: u32,
    ) -> Result<()>
    where
        T: std::io::Write + std::io::Seek,
    {
        let utc_timestamp = self.compute_utc_timestamp(&sample);
        self.h264_parser.push_nals(sample, Some(utc_timestamp));
        let sps = self.h264_parser.sps().unwrap();
        let pps = self.h264_parser.pps().unwrap();

        let mut mp4_writer = match std::mem::replace(mp4_segment, MaybeMp4Writer::Nothing) {
            MaybeMp4Writer::Mp4Writer(mp4_writer) => mp4_writer,
            MaybeMp4Writer::Starting(fd) => {
                start_mp4_writer(fd, sps, pps, trim_width, trim_height)?
            }
            MaybeMp4Writer::Nothing => {
                panic!("inconsistent state");
            }
        };

        let avcc_sample = self.h264_parser.avcc_sample().unwrap();
        mp4_writer.write_sample(TRACK_ID, &avcc_sample)?;

        *mp4_segment = MaybeMp4Writer::Mp4Writer(mp4_writer);

        Ok(())
    }
}

pub struct IOBuffer<I, O> {
    pub in_buf: I,
    pub out_buf: O,
}

enum MaybeMp4Writer<T>
where
    T: std::io::Write + std::io::Seek,
{
    Nothing,
    Starting(T),
    Mp4Writer(mp4::Mp4Writer<T>),
}

#[derive(Clone)]
struct H264Parser {
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    previous_stamp: Option<u64>,
    /// stores MP4 sample until written
    last_sample: Option<ParsedH264Frame>,
    first_frame_done: bool,
    h264_metadata: Option<H264Metadata>,
}

impl H264Parser {
    /// Create a new [H264Parser].
    fn new(h264_metadata: Option<H264Metadata>) -> Self {
        Self {
            sps: None,
            pps: None,
            previous_stamp: None,
            last_sample: None,
            first_frame_done: false,
            h264_metadata,
        }
    }
    fn sps(&self) -> Option<&[u8]> {
        self.sps.as_deref()
    }
    fn pps(&self) -> Option<&[u8]> {
        self.pps.as_deref()
    }

    fn push_nals(
        &mut self,
        nals: EbspNals,
        mut precision_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        // We assume that sample contains one or more compete NAL units and
        // starts with a NAL unit. Furthermore, we assume the start bytes can
        // only be [0x00, 0x00, 0x00, 0x01]. This is not a real Annex B parser
        // because of these assumptions but rather tuned to the output of
        // less-avc, nvenc and openh264 as we use them.

        let mut all_avcc_nal_units: Vec<u8> = Vec::with_capacity(nals.annex_b_size() + 32);

        if !self.first_frame_done {
            use less_avc::{nal_unit::*, sei::UserDataUnregistered};

            if let Some(h264_metadata) = &self.h264_metadata {
                // Update the `creation_time` field of the metadata with the
                // timestamp of the first frame.
                let h264_metadata_updated = if let Some(ts) = precision_timestamp {
                    // Keep the timezone from the existing metadata.
                    let tz = h264_metadata.creation_time.offset();
                    let creation_time = ts.with_timezone(tz);
                    H264Metadata {
                        creation_time,
                        ..h264_metadata.clone()
                    }
                } else {
                    h264_metadata.clone()
                };

                let msg = serde_json::to_vec(&h264_metadata_updated).unwrap();

                let payload = UserDataUnregistered::new(H264_METADATA_UUID, msg);

                use less_avc::sei::SupplementalEnhancementInformation;
                let rbsp_data =
                    SupplementalEnhancementInformation::UserDataUnregistered(payload).to_rbsp();
                let annex_b_data = NalUnit::new(
                    less_avc::nal_unit::NalRefIdc::Zero,
                    less_avc::nal_unit::NalUnitType::SupplementalEnhancementInformation,
                    rbsp_data,
                )
                .to_annex_b_data();

                const ANNEX_B_START: &[u8] = &[0x00, 0x00, 0x00, 0x01];
                debug_assert_eq!(&annex_b_data[..4], ANNEX_B_START);

                // Don't use the start code from Annex B but do use the raw EBSP
                // NALU.
                all_avcc_nal_units.extend(buf_to_avcc(&annex_b_data[4..]));
            }

            self.first_frame_done = true;
        }

        // Split into Encapsulated Byte Sequence Payload (EBSP) message
        for ebsp_msg in nals.nals.iter() {
            let mut did_sps_pps = false;
            if !ebsp_msg.is_empty() {
                let code = ebsp_msg[0];
                match code {
                    0x67 => {
                        self.sps = Some(ebsp_msg[..].to_vec());
                        did_sps_pps = true;
                    }
                    0x68 => {
                        self.pps = Some(ebsp_msg[..].to_vec());
                        did_sps_pps = true;
                    }
                    _ => {}
                }
                if !did_sps_pps {
                    // Don't insert the timestamp before SPS or PPS.
                    if let Some(ts) = precision_timestamp.take() {
                        let mut rbsp_msg = [0u8; 32];
                        rbsp_msg[0] = 0x06; // code 6 - SEI?
                        rbsp_msg[1] = 0x05; // header type: UserDataUnregistered
                        rbsp_msg[2] = 28; // size
                        timestamp_to_sei_payload(ts, &mut rbsp_msg[3..31]);
                        rbsp_msg[31] = 0x80; // rbsp_trailing_bits

                        // Create new NAL unit for precision timestamp. In
                        // theory we should ensure that this does not have start
                        // code bytes and thus we should convert from RBSP to
                        // EBSP. However, the standard ensures that there is no
                        // need for encoding and thus the RBSP is the EBSP for
                        // this case.
                        let ebsp_msg = rbsp_msg;
                        all_avcc_nal_units.extend(buf_to_avcc(&ebsp_msg[..]));
                    }
                }
                all_avcc_nal_units.extend(buf_to_avcc(ebsp_msg));
            }
        }

        if self
            .last_sample
            .replace(ParsedH264Frame {
                mp4_sample_start_time: nals.mp4_sample_start_time,
                is_keyframe: nals.is_keyframe,
                avcc_buf: all_avcc_nal_units,
            })
            .is_some()
        {
            eprintln!("unused NAL unit");
        };
    }

    fn avcc_sample(&mut self) -> Option<mp4::Mp4Sample> {
        let mut sample = self.last_sample.take().map(parsed_to_mp4_sample);
        if let Some(ref mut s) = sample {
            if let Some(prev) = self.previous_stamp {
                // FIXME: This will be off by one frame because it calculates duration
                // of this frame as delta between previous frame and this frame. (It
                // should be delta between this frame and next frame.)
                let dur = s.start_time - prev;
                s.duration = dur.try_into().unwrap();
            }
            self.previous_stamp = Some(s.start_time);
        }
        // Note: as far as I can tell, as of version 0.13.0, the mp4 crate does not
        // use `start_time` for writing the sample. (So we have gone to the trouble
        // of ensuring it has a good PTS value but it is ignored.)
        sample
    }
}

fn parsed_to_mp4_sample(orig: ParsedH264Frame) -> mp4::Mp4Sample {
    let bytes = orig.avcc_buf.into();

    mp4::Mp4Sample {
        start_time: orig.mp4_sample_start_time,
        duration: 0,
        rendering_offset: 0,
        is_sync: orig.is_keyframe,
        bytes,
    }
}

/// Encapsulated NAL Units
///
/// Stored neither in AnnexB nor AVCC format, just as buffers of encapsulated
/// bytes. A single MP4 sample can be composed of multiple such H264 NAL units.
struct EbspNals {
    pts: chrono::Duration,
    /// in units of `movie_timescale`
    mp4_sample_start_time: u64,
    is_keyframe: bool,
    nals: Vec<Vec<u8>>,
}

impl EbspNals {
    fn annex_b_size(&self) -> usize {
        let raw_sz: usize = self.nals.iter().map(|x| x.len()).sum();
        raw_sz + 4 * self.nals.len()
    }
}

#[derive(Clone)]
struct ParsedH264Frame {
    /// in units of `movie_timescale`
    mp4_sample_start_time: u64,
    is_keyframe: bool,
    avcc_buf: Vec<u8>,
}

fn buf_to_avcc(nal: &[u8]) -> Vec<u8> {
    let sz: u32 = nal.len().try_into().unwrap();
    let mut result = vec![0u8; nal.len() + 4];
    result[0..4].copy_from_slice(&sz.to_be_bytes());
    result[4..].copy_from_slice(nal);
    result
}

#[cfg(feature = "openh264")]
struct YUVData {
    width: usize,
    height: usize,
    data: Vec<u8>,
    y_stride: usize,
    u_stride: usize,
    v_stride: usize,
}

// fn print_buf(buf: &[u8]) {
//     use sha2::Digest;
//     let digest = sha2::Sha256::digest(&buf);
//     print!("buf: (chk {:x}, len {}) ", digest, buf.len());

//     let blen = buf.len().min(10);
//     for b in &buf[0..blen] {
//         print!("{:x} ", b);
//     }

//     println!("");
// }

// #[cfg(feature = "openh264")]
// impl std::fmt::Debug for YUVData {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
//         use sha2::Digest;
//         let digest = sha2::Sha256::digest(&self.data);
//         write!(
//             f,
//             "YUVData {{ width: {}, height: {}, data: (chk {:x}, len {}) ",
//             self.width,
//             self.height,
//             digest,
//             self.data.len()
//         )?;

//         let blen = self.data.len().min(10);
//         for b in &self.data[0..blen] {
//             write!(f, "{:x} ", b)?;
//         }

//         write!(f, ".. }}")
//     }
// }

#[cfg(feature = "openh264")]
impl From<convert_image::Y4MFrame> for YUVData {
    fn from(orig: convert_image::Y4MFrame) -> YUVData {
        let width = orig.width.try_into().unwrap();
        let height = orig.height.try_into().unwrap();
        let y_stride = orig.y_stride.try_into().unwrap();
        let u_stride = orig.u_stride();
        let v_stride = orig.v_stride();
        Self {
            width,
            height,
            data: orig.into_data(),
            y_stride,
            u_stride,
            v_stride,
        }
    }
}

#[cfg(feature = "openh264")]
impl YUVData {
    #[inline]
    fn u_start(&self) -> usize {
        self.height * self.y_stride
    }
    #[inline]
    fn v_start(&self) -> usize {
        self.u_start() + self.height / 2 * self.u_stride
    }
    #[inline]
    fn v_end(&self) -> usize {
        self.v_start() + self.height / 2 * self.u_stride
    }
}

#[cfg(feature = "openh264")]
impl openh264::formats::YUVSource for YUVData {
    fn width(&self) -> i32 {
        self.width.try_into().unwrap()
    }
    fn height(&self) -> i32 {
        self.height.try_into().unwrap()
    }
    fn y(&self) -> &[u8] {
        &self.data[0..self.u_start()]
    }
    fn u(&self) -> &[u8] {
        &self.data[self.u_start()..self.v_start()]
    }
    fn v(&self) -> &[u8] {
        &self.data[self.v_start()..self.v_end()]
    }
    fn y_stride(&self) -> i32 {
        self.y_stride.try_into().unwrap()
    }
    fn u_stride(&self) -> i32 {
        self.u_stride.try_into().unwrap()
    }
    fn v_stride(&self) -> i32 {
        self.v_stride.try_into().unwrap()
    }
}

fn dur2raw(dur: &std::time::Duration) -> u64 {
    (dur.as_secs_f64() * MOVIE_TIMESCALE as f64).round() as u64
}

fn timestamp_to_sei_payload(timestamp: chrono::DateTime<chrono::Utc>, payload: &mut [u8]) {
    assert_eq!(payload.len(), 28);
    let precision_time_stamp = timestamp.timestamp_micros();

    let precision_time_stamp_bytes: [u8; 8] = precision_time_stamp.to_be_bytes();

    payload[0..16].copy_from_slice(b"MISPmicrosectime"); // uuid_iso_iec_11578,

    payload[16] = 0x0F; // Time Stamp Status byte from MISB Standard 0604

    // The standard has 0xFF present after every two bytes as "Start Code
    // Emulation Prevention". This means that the raw byte sequence is identical
    // to the encoded byte sequence as there is nothing to encode.
    payload[17..19].copy_from_slice(&precision_time_stamp_bytes[0..2]);
    payload[19] = 0xff;
    payload[20..22].copy_from_slice(&precision_time_stamp_bytes[2..4]);
    payload[22] = 0xff;
    payload[23..25].copy_from_slice(&precision_time_stamp_bytes[4..6]);
    payload[25] = 0xff;
    payload[26..28].copy_from_slice(&precision_time_stamp_bytes[6..8]);
}

#[cfg(feature = "openh264")]
fn convert_openh264_rc_mode(
    orig: ci2_remote_control::OpenH264RateControlMode,
) -> openh264::encoder::RateControlMode {
    use ci2_remote_control::OpenH264RateControlMode as mode;
    use openh264::encoder::RateControlMode::*;
    match orig {
        mode::Quality => Quality,
        mode::Bitrate => Bitrate,
        mode::Bufferbased => Bufferbased,
        mode::Timestamp => Timestamp,
        mode::Off => Off,
    }
}

struct NalAvccBufIter<'a> {
    cur_buf: &'a [u8],
}

impl<'a> Iterator for NalAvccBufIter<'a> {
    type Item = Result<&'a [u8]>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur_buf.is_empty() {
            return None;
        }
        if self.cur_buf.len() < 4 {
            return Some(Err(Error::BadInputData {
                #[cfg(feature = "backtrace")]
                backtrace: std::backtrace::Backtrace::capture(),
            }));
        }
        let bytes: [u8; 4] = self.cur_buf[0..4].try_into().unwrap();
        let nal_unit_payload_len = usize::try_from(u32::from_be_bytes(bytes)).unwrap();
        let nal_ebsp_bytes = &self.cur_buf[4..4 + nal_unit_payload_len];
        if nal_ebsp_bytes.len() != nal_unit_payload_len {
            return Some(Err(Error::BadInputData {
                #[cfg(feature = "backtrace")]
                backtrace: std::backtrace::Backtrace::capture(),
            }));
        }
        self.cur_buf = &self.cur_buf[4 + nal_unit_payload_len..];
        Some(Ok(nal_ebsp_bytes))
    }
}

fn iter_avcc_bufs(buf: &[u8]) -> NalAvccBufIter {
    NalAvccBufIter { cur_buf: buf }
}

/// parse h264 NAL unit and return if it is an IDR frame
fn parse_h264_is_idr_frame(data: &frame_source::H264EncodingVariant) -> Result<bool> {
    use h264_reader::nal::{Nal, RefNal, UnitType};
    use h264_reader::push::NalInterest;
    let mut calls = Vec::new();
    match data {
        frame_source::H264EncodingVariant::Avcc(buf) => {
            let nal_iter = iter_avcc_bufs(buf);
            for nal_ebsp_bytes in nal_iter {
                let nal_ebsp_bytes = nal_ebsp_bytes?;
                let nal = RefNal::new(nal_ebsp_bytes, &[], true);
                let nal_unit_type = nal.header().unwrap().nal_unit_type();
                calls.push(nal_unit_type);
            }
        }
        frame_source::H264EncodingVariant::AnnexB(buf) => {
            use h264_reader::annexb::AnnexBReader;
            let mut reader = AnnexBReader::accumulate(|nal: RefNal<'_>| {
                let nal_unit_type = nal.header().unwrap().nal_unit_type();
                calls.push(nal_unit_type);
                match nal_unit_type {
                    UnitType::SeqParameterSet => NalInterest::Buffer,
                    _ => NalInterest::Ignore,
                }
            });
            reader.push(&buf[..]);
        }
        frame_source::H264EncodingVariant::RawEbsp(nals) => {
            for nal_ebsp_bytes in nals.iter() {
                let nal = RefNal::new(nal_ebsp_bytes, &[], true);
                let nal_unit_type = nal.header().unwrap().nal_unit_type();
                calls.push(nal_unit_type);
            }
        }
    }
    let mut is_keyframe = None;
    for nal_unit_type in calls.into_iter() {
        match nal_unit_type {
            UnitType::SliceLayerWithoutPartitioningIdr => {
                if is_keyframe.is_some() {
                    // cannot have multiple frames
                    return Err(Error::BadInputData {
                        #[cfg(feature = "backtrace")]
                        backtrace: std::backtrace::Backtrace::capture(),
                    });
                };
                is_keyframe = Some(true);
            }
            UnitType::SliceLayerWithoutPartitioningNonIdr => {
                if is_keyframe.is_some() {
                    // cannot have multiple frames
                    return Err(Error::BadInputData {
                        #[cfg(feature = "backtrace")]
                        backtrace: std::backtrace::Backtrace::capture(),
                    });
                };
                is_keyframe = Some(false);
            }
            _ => {}
        }
    }
    #[allow(clippy::unnecessary_lazy_evaluations)]
    is_keyframe.ok_or_else(|| Error::BadInputData {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace::capture(),
    })
}

fn inconsistent_state_err<T>() -> Result<T> {
    Err(Error::InconsistentState {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace::capture(),
    })
}
