#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access, provide_any)
)]

use std::rc::Rc;

#[macro_use]
extern crate log;

use ci2_remote_control::Mp4RecordingConfig;
use convert_image::convert_into;

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};

use machine_vision_formats::{
    pixel_format::NV12, ImageBuffer, ImageBufferMutRef, ImageBufferRef, ImageData, ImageStride,
    PixelFormat, Stride,
};
use nvenc::{InputBuffer, OutputBuffer, RateControlMode};

use thiserror::Error;

/// Convert to runtime specified pixel format and save to FMF file.
macro_rules! convert_and_write_mp4 {
    ($new_pixel_format:expr, $writer:expr, $x:expr, $timestamp:expr) => {{
        use machine_vision_formats::pixel_format::*;
        match $new_pixel_format {
            PixFmt::Mono8 => write_converted!(Mono8, $writer, $x, $timestamp),
            PixFmt::Mono32f => write_converted!(Mono32f, $writer, $x, $timestamp),
            PixFmt::RGB8 => write_converted!(RGB8, $writer, $x, $timestamp),
            PixFmt::BayerRG8 => write_converted!(BayerRG8, $writer, $x, $timestamp),
            PixFmt::BayerRG32f => write_converted!(BayerRG32f, $writer, $x, $timestamp),
            PixFmt::BayerGB8 => write_converted!(BayerGB8, $writer, $x, $timestamp),
            PixFmt::BayerGB32f => write_converted!(BayerGB32f, $writer, $x, $timestamp),
            PixFmt::BayerGR8 => write_converted!(BayerGR8, $writer, $x, $timestamp),
            PixFmt::BayerGR32f => write_converted!(BayerGR32f, $writer, $x, $timestamp),
            PixFmt::BayerBG8 => write_converted!(BayerBG8, $writer, $x, $timestamp),
            PixFmt::BayerBG32f => write_converted!(BayerBG32f, $writer, $x, $timestamp),
            PixFmt::YUV422 => write_converted!(YUV422, $writer, $x, $timestamp),
            _ => {
                return Err(Error::UnsupportedConversion {
                    #[cfg(feature = "backtrace")]
                    backtrace: std::backtrace::Backtrace::capture(),
                });
            }
        }
    }};
}

/// For a specified runtime specified pixel format, convert and save to FMF file.
macro_rules! write_converted {
    ($pixfmt:ty, $writer:expr, $x:expr, $timestamp:expr) => {{
        let converted_frame = convert_image::convert::<_, $pixfmt>($x)?;
        $writer.write(&converted_frame, $timestamp)?;
    }};
}

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
    #[error("file already closed")]
    FileAlreadyClosed,
    #[error("inconsistent state")]
    InconsistentState,
    #[error("timestamp too large")]
    TimestampTooLarge,
    #[error("unsupported conversion")]
    UnsupportedConversion {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("image is padded")]
    StrideError {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
    },
    #[error("convert image error")]
    ConvertImageError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        convert_image::Error,
    ),
    #[cfg(feature = "open-h264")]
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
    Nvidia(NvEncoder<'lib>),
    #[cfg(feature = "open-h264")]
    OpenH264(OpenH264Encoder),
    // Uncompressed(UncompressedEncoder),
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

pub fn trim_image<FMT>(orig: &dyn ImageStride<FMT>, width: u32, height: u32) -> TrimmedImage<FMT> {
    TrimmedImage {
        orig,
        width,
        height,
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
        Ok(Self {
            inner: Some(WriteState::Configured((fd, config))),
            nv_enc,
        })
    }

    pub fn write_dynamic(
        &mut self,
        frame: &DynamicFrame,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let fmt = frame.pixel_format();
        match_all_dynamic_fmts!(frame, x, convert_and_write_mp4!(fmt, self, x, timestamp));
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
            Some(WriteState::Configured((fd, cfg))) => {
                // This doesn't actually trim the image but still
                // wraps the image in a TrimmedImage wrapper.
                let frame = match cfg.codec {
                    _ => {
                        // This path doesn't actually trim the image but still
                        // wraps the image in a TrimmedImage wrapper.
                        trim_image(frame, frame.width(), frame.height())
                    }
                };

                let width = frame.width();
                let height = frame.height();

                let mut opt_nv_h264_encoder = None;

                match &cfg.codec {
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
                                    h264_parser: Default::default(),
                                    // annex_b_reader,
                                    vram_queue,
                                    first_timestamp: timestamp,
                                });
                            }
                            None => return Err(Error::NvencLibsNotLoaded),
                        }
                    }
                };

                let track_id = 1;

                let my_encoder = match cfg.codec {
                    ci2_remote_control::Mp4Codec::H264OpenH264(_) => {
                        #[cfg(feature = "open-h264")]
                        {
                            let cfg = openh264::encoder::EncoderConfig::new(width, height)
                                // // .debug(true)
                                .enable_skip_frame(false)
                                .max_frame_rate(1_000_000.0)
                                // .rate_control_mode(
                                //     openh264::encoder::RateControlMode::BitrateModePostSkip,
                                // )
                                .rate_control_mode(openh264::encoder::RateControlMode::Bufferbased)
                                // .rate_control_mode(openh264::encoder::RateControlMode::Off)
                                // .set_bitrate_bps(10_000_000);
                                .set_bitrate_bps(100_000);

                            MyEncoder::OpenH264(OpenH264Encoder {
                                encoder: openh264::encoder::Encoder::with_config(cfg)?,
                                h264_parser: Default::default(),
                                first_timestamp: timestamp,
                            })
                        }
                        #[cfg(not(feature = "open-h264"))]
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

                let mut state = RecordingState {
                    mp4_segment: MaybeMp4Writer::Starting(fd),
                    track_id,
                    my_encoder,
                    first_timestamp: timestamp,
                    previous_timestamp: timestamp,
                    target_interval: chrono::Duration::from_std(cfg.max_framerate.interval())
                        .unwrap(),
                    trim_width: width,
                    trim_height: height,
                };

                write_frame(&mut state, &frame, timestamp)?;

                self.inner = Some(WriteState::Recording(state));

                Ok(())
            }
            Some(WriteState::Recording(mut state)) => {
                let interval = timestamp.signed_duration_since(state.previous_timestamp);
                if interval >= state.target_interval {
                    let frame = trim_image(frame, state.trim_width, state.trim_height);
                    debug!("Saving frame at {}: interval {}", timestamp, interval);
                    write_frame(&mut state, &frame, timestamp)?;
                    state.previous_timestamp = timestamp;
                } else {
                    debug!(
                        "Not saving frame at {}: interval {} too small",
                        timestamp, interval
                    );
                }

                self.inner = Some(WriteState::Recording(state));

                Ok(())
            }
            Some(WriteState::Finished) => {
                self.inner = Some(WriteState::Finished);
                Err(Error::FileAlreadyClosed)
            }

            None => Err(Error::InconsistentState),
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
            Some(WriteState::Configured((_fd, _cfg))) => {
                // no frames written.
                self.inner = Some(WriteState::Finished);
                Ok(())
            }
            Some(WriteState::Recording(mut state)) => {
                match state.my_encoder {
                    #[cfg(feature = "open-h264")]
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
                            nv_encoder.inner_save_data(
                                state.track_id,
                                &mut state.mp4_segment,
                                sample,
                                state.trim_width,
                                state.trim_height,
                            )?;
                        }
                    }
                }

                match state.mp4_segment {
                    MaybeMp4Writer::Mp4Writer(mut mp4_writer) => {
                        mp4_writer.write_end()?;
                    }
                    _ => {}
                }

                trace!("Finalized video.");
                self.inner = Some(WriteState::Finished);
                Ok(())
            }
            Some(WriteState::Finished) => {
                self.inner = Some(WriteState::Finished);
                Err(Error::FileAlreadyClosed)
            }
            None => Err(Error::InconsistentState),
        }
    }
}

fn nv_outbuf_to_sample(outbuf: dynlink_nvidia_encode::api::LockedOutputBuffer) -> AnnexBSample {
    let bytes: Vec<u8> = outbuf.mem().to_vec(); // copy data
    AnnexBSample {
        pts: chrono::Duration::from_std(outbuf.pts().clone()).unwrap(),
        output_time_stamp: outbuf.output_time_stamp(),
        is_keyframe: outbuf.is_keyframe(),
        bytes,
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
            // Happy path when .finished() not already called.
            Some(_) => self.finish().unwrap(),
        }
    }
}

trait PtsDur {
    fn pts_dur(&self) -> std::time::Duration;
}

fn write_frame<'lib, T, FRAME, FMT>(
    state: &mut RecordingState<'lib, T>,
    raw_frame: &FRAME,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<()>
where
    T: std::io::Write + std::io::Seek,
    FRAME: ImageStride<FMT>,
    FMT: PixelFormat,
{
    let elapsed = timestamp.signed_duration_since(state.first_timestamp);

    match &mut state.my_encoder {
        #[cfg(feature = "open-h264")]
        MyEncoder::OpenH264(encoder) => {
            // todo: bitrate, keyframes, timestamp check and duration finding.

            let y4m = convert_image::encode_y4m_frame(
                raw_frame,
                convert_image::Y4MColorspace::C420paldv,
            )?;
            let blarg = Blarg::from(y4m);

            let encoded = encoder.encoder.encode(&blarg).unwrap();

            use openh264::encoder::FrameType;
            let is_keyframe =
                (encoded.frame_type() == FrameType::IDR) | (encoded.frame_type() == FrameType::I);

            let bytes = encoded.to_vec();
            // if bytes.len() == 0 {
            //     panic!("did not encode frame!?");
            // }

            let pts = timestamp - encoder.first_timestamp;
            let output_time_stamp = dur2raw(&pts.to_std().unwrap());

            let sample = AnnexBSample {
                pts,
                output_time_stamp,
                is_keyframe,
                bytes,
            };

            encoder.inner_save_data(
                state.track_id,
                &mut state.mp4_segment,
                sample,
                state.trim_width,
                state.trim_height,
            )?;
        }
        MyEncoder::Nvidia(ref mut nv_encoder) => {
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
                        state.track_id,
                        &mut state.mp4_segment,
                        sample,
                        state.trim_width,
                        state.trim_height,
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
                let dptr = inbuf.mem_mut();
                let mut dest: ImageBufferMutRef<NV12> = ImageBufferMutRef::new(dptr);
                convert_into(raw_frame, &mut dest, dest_stride)?;
                // Now vram_buf.in_buf has the nv12 encoded data.
                dest_stride
            };

            let pts = elapsed.to_std().unwrap();

            nv_encoder
                .encoder
                .encode_picture(&vram_buf.in_buf, &vram_buf.out_buf, pitch, pts)?;
        }
    }
    Ok(())
}

enum WriteState<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    Configured((T, Mp4RecordingConfig)),
    Recording(RecordingState<'lib, T>),
    Finished,
}

struct RecordingState<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    mp4_segment: MaybeMp4Writer<T>,
    track_id: u32,
    my_encoder: MyEncoder<'lib>,
    first_timestamp: chrono::DateTime<chrono::Utc>,
    previous_timestamp: chrono::DateTime<chrono::Utc>,
    target_interval: chrono::Duration,
    trim_width: u32,
    trim_height: u32,
}

struct NvEncoder<'lib> {
    encoder: Rc<nvenc::Encoder<'lib>>,
    h264_parser: H264Parser,
    vram_queue: nvenc::Queue<IOBuffer<InputBuffer<'lib>, OutputBuffer<'lib>>>,
    first_timestamp: chrono::DateTime<chrono::Utc>,
}

impl<'lib> NvEncoder<'lib> {
    fn compute_utc_timestamp(&self, sample: &AnnexBSample) -> chrono::DateTime<chrono::Utc> {
        self.first_timestamp + sample.pts
    }
    fn inner_save_data<T>(
        &mut self,
        track_id: u32,
        mp4_segment: &mut MaybeMp4Writer<T>,
        sample: AnnexBSample,
        trim_width: u32,
        trim_height: u32,
    ) -> Result<()>
    where
        T: std::io::Write + std::io::Seek,
    {
        let utc_timestamp = self.compute_utc_timestamp(&sample);
        self.h264_parser.push_annex_b(sample, Some(utc_timestamp));
        let mut mp4_writer = match std::mem::replace(mp4_segment, MaybeMp4Writer::Nothing) {
            MaybeMp4Writer::Mp4Writer(mp4_writer) => mp4_writer,
            MaybeMp4Writer::Starting(fd) => {
                let sps = self.h264_parser.sps().unwrap();
                let pps = self.h264_parser.pps().unwrap();

                let mp4_config = mp4::Mp4Config {
                    major_brand: str::parse("isom").unwrap(),
                    minor_version: 512,
                    compatible_brands: vec![
                        str::parse("isom").unwrap(),
                        str::parse("iso2").unwrap(),
                        str::parse("avc1").unwrap(),
                        str::parse("mp41").unwrap(),
                    ],
                    timescale: 1000, // fixme
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
                    timescale: dynlink_nvidia_encode::api::H264_RATE,
                    language: String::from("und"),
                    media_conf,
                };

                mp4_writer.add_track(&track_conf)?;
                mp4_writer
            }
            MaybeMp4Writer::Nothing => {
                panic!("inconsistent state");
            }
        };

        let avcc_sample = self.h264_parser.avcc_sample().unwrap();
        mp4_writer.write_sample(track_id, &avcc_sample.inner)?;

        *mp4_segment = MaybeMp4Writer::Mp4Writer(mp4_writer);

        Ok(())
    }
}

#[cfg(feature = "open-h264")]
struct OpenH264Encoder {
    encoder: openh264::encoder::Encoder,
    h264_parser: H264Parser,
    first_timestamp: chrono::DateTime<chrono::Utc>,
}

#[cfg(feature = "open-h264")]
impl OpenH264Encoder {
    fn compute_utc_timestamp(&self, sample: &AnnexBSample) -> chrono::DateTime<chrono::Utc> {
        self.first_timestamp + sample.pts
    }
    fn inner_save_data<T>(
        &mut self,
        track_id: u32,
        mp4_segment: &mut MaybeMp4Writer<T>,
        sample: AnnexBSample,
        trim_width: u32,
        trim_height: u32,
    ) -> Result<()>
    where
        T: std::io::Write + std::io::Seek,
    {
        let utc_timestamp = self.compute_utc_timestamp(&sample);
        self.h264_parser.push_annex_b(sample, Some(utc_timestamp));
        let sps = self.h264_parser.sps().unwrap();
        let pps = self.h264_parser.pps().unwrap();

        let mut mp4_writer = match std::mem::replace(mp4_segment, MaybeMp4Writer::Nothing) {
            MaybeMp4Writer::Mp4Writer(mp4_writer) => mp4_writer,
            MaybeMp4Writer::Starting(fd) => {
                let mp4_config = mp4::Mp4Config {
                    major_brand: str::parse("isom").unwrap(),
                    minor_version: 512,
                    compatible_brands: vec![
                        str::parse("isom").unwrap(),
                        str::parse("iso2").unwrap(),
                        str::parse("avc1").unwrap(),
                        str::parse("mp41").unwrap(),
                    ],
                    timescale: 1000, // fixme
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
                    timescale: dynlink_nvidia_encode::api::H264_RATE,
                    language: String::from("eng"),
                    media_conf,
                };

                mp4_writer.add_track(&track_conf)?;
                mp4_writer
            }
            MaybeMp4Writer::Nothing => {
                panic!("inconsistent state");
            }
        };

        let avcc_sample = self.h264_parser.avcc_sample().unwrap();
        mp4_writer.write_sample(track_id, &avcc_sample.inner)?;

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

#[derive(Default)]
struct H264Parser {
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    last_sample: Option<HmmSample>,
}

impl H264Parser {
    fn sps(&self) -> Option<&[u8]> {
        self.sps.as_ref().map(|x| x.as_slice())
    }
    fn pps(&self) -> Option<&[u8]> {
        self.pps.as_ref().map(|x| x.as_slice())
    }

    fn push_annex_b(
        &mut self,
        sample: AnnexBSample,
        mut precision_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    ) {
        // We assume that sample contains one or more complete NAL units and
        // starts with a NAL unit. Furthermore, we assume the start bytes can
        // only be [0x00, 0x00, 0x00, 0x01]. This is not a real Annex B parser
        // because of these assumptions but rather tuned to the output of nvenc
        // and openh264 as we use them.

        let mut all_avcc_nal_units: Vec<u8> = Vec::with_capacity(sample.bytes.len() + 32);

        for raw_nalu in my_split(&sample.bytes[..], &[0x00, 0x00, 0x00, 0x01]) {
            let mut did_sps_pps = false;
            if raw_nalu.len() > 0 {
                let code = raw_nalu[0];
                match code {
                    0x67 => {
                        self.sps = Some(raw_nalu[..].to_vec());
                        did_sps_pps = true;
                    }
                    0x68 => {
                        self.pps = Some(raw_nalu[..].to_vec());
                        did_sps_pps = true;
                    }
                    _ => {}
                }
                if !did_sps_pps {
                    if let Some(ts) = precision_timestamp.take() {
                        // Create new NAL unit for precision timestamp.
                        let mut raw_nalu = [0u8; 32];
                        raw_nalu[0] = 0x06; // code 6 - SEI?
                        raw_nalu[1] = 0x05; // header type: UserDataUnregistered
                        raw_nalu[2] = 28; // size
                        raw_nalu[31] = 128; // ??
                        timestamp_to_sei_payload(ts, &mut raw_nalu[3..31]);
                        all_avcc_nal_units.extend(buf_to_avcc(&raw_nalu[..]));
                    }
                }
                all_avcc_nal_units.extend(buf_to_avcc(&raw_nalu[..]));
            }
        }

        if self
            .last_sample
            .replace(HmmSample {
                output_time_stamp: sample.output_time_stamp,
                is_keyframe: sample.is_keyframe,
                avcc_buf: all_avcc_nal_units,
            })
            .is_some()
        {
            eprintln!("unused NAL unit");
        };
    }

    fn avcc_sample(&mut self) -> Option<AvccSample> {
        self.last_sample.take().map(annex_b_to_avcc)
    }
}

fn annex_b_to_avcc(orig: HmmSample) -> AvccSample {
    let bytes = mp4::Bytes::copy_from_slice(&orig.avcc_buf[..]);

    AvccSample {
        inner: mp4::Mp4Sample {
            start_time: orig.output_time_stamp,
            duration: 5000, //fixme
            rendering_offset: 0,
            is_sync: orig.is_keyframe,
            bytes,
        },
    }
}

struct AnnexBSample {
    pts: chrono::Duration,
    output_time_stamp: u64,
    is_keyframe: bool,
    bytes: Vec<u8>,
}

struct HmmSample {
    output_time_stamp: u64,
    is_keyframe: bool,
    // nal_buf: Vec<u8>,
    avcc_buf: Vec<u8>,
}

struct AvccSample {
    inner: mp4::Mp4Sample,
}

fn buf_to_avcc(nal: &[u8]) -> Vec<u8> {
    let sz: u32 = nal.len().try_into().unwrap();
    let header: [u8; 4] = sz.to_be_bytes();
    let mut result = header.to_vec();
    result.extend(nal);
    result
}

fn my_split<'a, 'b>(large_buf: &'a [u8], sep: &'b [u8]) -> impl Iterator<Item = &'a [u8]> {
    let mut starts: Vec<usize> = Vec::new();
    if large_buf.len() < sep.len() {
        return Vec::new().into_iter();
    }
    for i in 0..large_buf.len() - sep.len() {
        if &large_buf[i..i + sep.len()] == &sep[..] {
            starts.push(i);
        }
    }
    // dbg!(&starts);
    let mut result: Vec<&'a [u8]> = Vec::with_capacity(starts.len());
    for window in starts.windows(2) {
        // dbg!(window);
        let window_start = window[0];
        let window_end = window[1];
        result.push(&large_buf[window_start + sep.len()..window_end]);
    }
    if !starts.is_empty() {
        let window_start = starts[starts.len() - 1];
        // dbg!(window_start);
        // dbg!(window_start+sep.len());
        // dbg!(large_buf.len());
        if window_start + sep.len() <= large_buf.len() {
            result.push(&large_buf[window_start + sep.len()..])
        }
    }
    result.into_iter()
}

#[cfg(feature = "open-h264")]
struct Blarg {
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

// #[cfg(feature = "open-h264")]
// impl std::fmt::Debug for Blarg {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
//         use sha2::Digest;
//         let digest = sha2::Sha256::digest(&self.data);
//         write!(
//             f,
//             "Blarg {{ width: {}, height: {}, data: (chk {:x}, len {}) ",
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

#[cfg(feature = "open-h264")]
impl From<convert_image::Y4MFrame> for Blarg {
    fn from(orig: convert_image::Y4MFrame) -> Blarg {
        let width = orig.width.try_into().unwrap();
        let height = orig.height.try_into().unwrap();
        let y_stride = orig.y_stride.try_into().unwrap();
        let u_stride = orig.u_stride.try_into().unwrap();
        let v_stride = orig.v_stride.try_into().unwrap();
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

#[cfg(feature = "open-h264")]
impl Blarg {
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

#[cfg(feature = "open-h264")]
impl openh264::formats::YUVSource for Blarg {
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

#[test]
fn test_split() {
    let results: Vec<&[u8]> = my_split(
        &[0, 0, 0, 1, 9, 10, 10, 0, 0, 0, 1, 3, 20, 0, 0, 0, 1, 99, 99],
        &[0, 0, 0, 1],
    )
    .collect();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], &[9, 10, 10]);
    assert_eq!(results[1], &[3, 20]);
    assert_eq!(results[2], &[99, 99]);

    let results: Vec<&[u8]> = my_split(&[], &[0, 0, 0, 1]).collect();
    assert_eq!(results.len(), 0);
}

#[cfg(feature = "open-h264")]
fn dur2raw(dur: &std::time::Duration) -> u64 {
    (dur.as_secs_f64() * H264_RATE as f64).round() as u64
}

pub const H264_RATE: u32 = 1_000_000;

fn timestamp_to_sei_payload(timestamp: chrono::DateTime<chrono::Utc>, payload: &mut [u8]) {
    assert_eq!(payload.len(), 28);
    let precision_time_stamp = timestamp.timestamp_micros();

    let precision_time_stamp_bytes: [u8; 8] = precision_time_stamp.to_be_bytes();

    payload[0..16].copy_from_slice(b"MISPmicrosectime"); // uuid_iso_iec_11578,

    payload[16] = 0x0F; // Time Stamp Status byte from MISB Standard 0604
    payload[17..19].copy_from_slice(&precision_time_stamp_bytes[0..2]);
    payload[19] = 0xff;
    payload[20..22].copy_from_slice(&precision_time_stamp_bytes[2..4]);
    payload[22] = 0xff;
    payload[23..25].copy_from_slice(&precision_time_stamp_bytes[4..6]);
    payload[25] = 0xff;
    payload[26..28].copy_from_slice(&precision_time_stamp_bytes[6..8]);
}
