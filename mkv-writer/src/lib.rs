#![cfg_attr(feature = "backtrace", feature(backtrace))]

use std::rc::Rc;

#[macro_use]
extern crate log;

use ci2_remote_control::MkvRecordingConfig;
use convert_image::encode_into_nv12;

#[cfg(feature = "vpx")]
use convert_image::{encode_y4m_frame, Y4MColorspace};

use machine_vision_formats::{ImageBufferMutRef, ImageStride, PixelFormat};
use nvenc::{InputBuffer, OutputBuffer, RateControlMode};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
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
    #[error("convert image error")]
    ConvertImageError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        convert_image::Error,
    ),
    #[cfg(feature = "vpx")]
    #[error("VPX Encoder Error")]
    VpxEncoderError {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        inner: vpx_encode::Error,
    },
    #[error("Compiled without VPX support but VPX requested")]
    NoVpxAvailable {
        #[cfg(feature = "backtrace")]
        backtrace: std::backtrace::Backtrace,
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
    #[cfg(feature = "vpx")]
    Vpx(vpx_encode::Encoder),
    Nvidia(NvEncoder<'lib>),
}

pub struct MkvWriter<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    inner: Option<WriteState<'lib, T>>,
    nv_enc: Option<nvenc::NvEnc<'lib>>,
    writing_application: String,
}

impl<'lib, T> MkvWriter<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    pub fn new(
        fd: T,
        config: MkvRecordingConfig,
        nv_enc: Option<nvenc::NvEnc<'lib>>,
    ) -> Result<Self> {
        let writing_application: String = config
            .clone()
            .writing_application
            .unwrap_or_else(|| format!("{}-{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
        Ok(Self {
            inner: Some(WriteState::Configured((fd, config))),
            nv_enc,
            writing_application,
        })
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
                use webm::mux;

                let width = frame.width();
                let height = frame.height();

                let mut mkv_segment =
                    mux::Segment::new(mux::Writer::new(fd)).expect("mux::Segment::new");

                #[allow(unused_assignments)]
                let mut opt_h264_encoder = None;

                let (vpx_tup, mux_codec) = match cfg.codec {
                    #[cfg(feature = "vpx")]
                    ci2_remote_control::MkvCodec::VP8(opts) => (
                        #[cfg(feature = "vpx")]
                        Some((vpx_encode::VideoCodecId::VP8, opts.bitrate)),
                        webm::mux::VideoCodecId::VP8,
                    ),
                    #[cfg(feature = "vpx")]
                    ci2_remote_control::MkvCodec::VP9(opts) => (
                        Some((vpx_encode::VideoCodecId::VP9, opts.bitrate)),
                        webm::mux::VideoCodecId::VP9,
                    ),
                    #[cfg(not(feature = "vpx"))]
                    ci2_remote_control::MkvCodec::VP8(_) | ci2_remote_control::MkvCodec::VP9(_) => {
                        return Err(Error::NoVpxAvailable {
                            #[cfg(feature = "backtrace")]
                            backtrace: std::backtrace::Backtrace::capture(),
                        });
                    }
                    ci2_remote_control::MkvCodec::H264(opts) => {
                        // scope for anonymous lifetime of ref
                        match &self.nv_enc {
                            Some(ref nv_enc) => {
                                debug!("Using codec H264 in mkv file.");

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
                                    param_builder.set_encode_config(encoder_config).build();

                                encoder.initialize(&params)?;

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

                                opt_h264_encoder = Some(NvEncoder {
                                    encoder,
                                    vram_queue,
                                });
                                (None, webm::mux::VideoCodecId::H264)
                            }
                            None => return Err(Error::NvencLibsNotLoaded),
                        }
                    }
                };

                let vt = mkv_segment.add_video_track(width, height, None, mux_codec);

                // A dummy type which is never used so the compiler does not complain.
                #[cfg(not(feature = "vpx"))]
                #[allow(unused_variables)]
                let vpx_tup: Option<u8> = vpx_tup;

                let my_encoder = if let Some(vpx_tup) = vpx_tup {
                    #[cfg(feature = "vpx")]
                    {
                        let (vpx_codec, bitrate) = vpx_tup;
                        debug!("Using codec {:?} in mkv file.", vpx_codec);
                        // Setup the encoder.
                        let vpx_encoder = vpx_encode::Encoder::new(vpx_encode::Config {
                            width: width,
                            height: height,
                            timebase: [1, 1000], // millisecond time base
                            bitrate,
                            codec: vpx_codec,
                        })?;

                        MyEncoder::Vpx(vpx_encoder)
                    }
                    #[cfg(not(feature = "vpx"))]
                    {
                        // We should never get here.
                        panic!("No VPX support at compilation time. VPX: {}", vpx_tup);
                    }
                } else {
                    let enc = opt_h264_encoder.unwrap();
                    MyEncoder::Nvidia(enc)
                };

                // Set DateUTC metadata
                use chrono::TimeZone;
                let millennium_exploded = chrono::Utc.ymd(2001, 1, 1).and_hms(0, 0, 0);
                let elapsed = timestamp.signed_duration_since(millennium_exploded);
                let nanoseconds = elapsed.num_nanoseconds().expect("nanosec overflow");
                // https://chromium.googlesource.com/chromium/src/+/11d989c52c6da43c5e8eb9d377ef0286a1cc8fba/remoting/client/plugin/media_source_video_renderer.cc
                // https://groups.google.com/a/chromium.org/forum/#!msg/chromium-reviews/DGrjsJm8TEk/YTQxIhUaz3MJ
                // "DateUTC is specified in nanoseconds from 0:00 on January 1st, 2001." in remoting/client/plugin/media_source_video_renderer.cc

                // Also see http://ffmpeg.org/doxygen/3.2/matroskaenc_8c_source.html
                debug!(
                    "saving DateUTC with value in mkv file: {} (from initial timestamp {})",
                    nanoseconds, timestamp
                );
                mkv_segment.set_date_utc(nanoseconds);
                mkv_segment.set_app_name(&self.writing_application);

                let mut state = RecordingState {
                    mkv_segment,
                    vt,
                    my_encoder,
                    first_timestamp: timestamp,
                    previous_timestamp: timestamp,
                    target_interval: chrono::Duration::from_std(cfg.max_framerate.interval())
                        .unwrap(),
                };

                write_frame(&mut state, frame, timestamp)?;

                self.inner = Some(WriteState::Recording(state));

                Ok(())
            }
            Some(WriteState::Recording(mut state)) => {
                let interval = timestamp.signed_duration_since(state.previous_timestamp);
                if interval >= state.target_interval {
                    debug!("Saving frame at {}: interval {}", timestamp, interval);
                    write_frame(&mut state, frame, timestamp)?;
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

    pub fn finish(&mut self) -> Result<()> {
        use webm::mux::Track;

        let inner = self.inner.take();
        match inner {
            Some(WriteState::Configured((_fd, _cfg))) => {
                // no frames written.
                self.inner = Some(WriteState::Finished);
                Ok(())
            }
            Some(WriteState::Recording(mut state)) => {
                match state.my_encoder {
                    #[cfg(feature = "vpx")]
                    MyEncoder::Vpx(vpx_encoder) => {
                        let mut frames = vpx_encoder.finish().unwrap();
                        trace!("Finishing vpx encoding.");
                        while let Some(frame) = frames.next().unwrap() {
                            state
                                .vt
                                .add_frame(frame.data, nanos(&frame.pts_dur()), frame.key);
                            trace!(
                                "got vpx encoded data for final frame(s): {} bytes",
                                frame.data.len()
                            );
                        }
                    }
                    MyEncoder::Nvidia(mut nv_encoder) => {
                        // Now done with all frames, drain the pending data.
                        loop {
                            match nv_encoder.vram_queue.get_pending() {
                                None => break,
                                Some(iobuf) => {
                                    // scope for locked output buffer
                                    let outbuf = iobuf.out_buf.lock()?;
                                    state.vt.add_frame(
                                        outbuf.mem(),
                                        nanos(outbuf.pts()),
                                        outbuf.is_keyframe(),
                                    );
                                }
                            }
                        }
                    }
                }

                // If duration is set to `None`, libwebm will set it
                // automatically.
                let _ = state.mkv_segment.finalize(None);

                trace!("Finalized mkv.");
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

trait PtsDur {
    fn pts_dur(&self) -> std::time::Duration;
}

#[cfg(feature = "vpx")]
impl<'a> PtsDur for vpx_encode::Frame<'a> {
    fn pts_dur(&self) -> std::time::Duration {
        // millisecond time base
        let secs = self.pts as f64 / 1000.0;
        std::time::Duration::from_secs_f64(secs)
    }
}

// remove once we have rust 1.38 everywhere and use dur.as_secs_f64()
fn as_secs_f64(dur: &std::time::Duration) -> f64 {
    dur.as_secs() as f64 + (dur.subsec_nanos() as f64 * 1e-9)
}

fn nanos(dur: &std::time::Duration) -> u64 {
    (as_secs_f64(dur) * 1e9).round() as u64
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
    use webm::mux::Track;

    let elapsed = timestamp.signed_duration_since(state.first_timestamp);

    match &mut state.my_encoder {
        #[cfg(feature = "vpx")]
        MyEncoder::Vpx(ref mut vpx_encoder) => {
            let yuv = encode_y4m_frame(raw_frame, Y4MColorspace::C420paldv)?;
            trace!("got yuv data for frame. {} bytes.", yuv.len());

            let milliseconds = elapsed.num_milliseconds();
            for frame in vpx_encoder.encode(milliseconds, &yuv).unwrap() {
                trace!("got vpx encoded data: {} bytes.", frame.data.len());
                state
                    .vt
                    .add_frame(frame.data, nanos(&frame.pts_dur()), frame.key);
            }
        }
        MyEncoder::Nvidia(ref mut nv_encoder) => {
            let vram_buf: &mut IOBuffer<_, _> = match nv_encoder.vram_queue.get_available() {
                Some(iobuf) => iobuf,
                None => {
                    let iobuf = nv_encoder.vram_queue.get_pending().expect("get pending");
                    {
                        // scope for locked output buffer
                        let outbuf = iobuf.out_buf.lock()?;
                        state
                            .vt
                            .add_frame(outbuf.mem(), nanos(outbuf.pts()), outbuf.is_keyframe());
                    }
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
                let mut dest = ImageBufferMutRef::new(dptr);
                encode_into_nv12(raw_frame, &mut dest, dest_stride)?;
                // Now vram_buf.in_buf has the nv12 encoded data.
                dest_stride
            };

            nv_encoder.encoder.encode_picture(
                &vram_buf.in_buf,
                &vram_buf.out_buf,
                pitch,
                elapsed.to_std().unwrap(),
            )?;
        }
    }
    Ok(())
}

enum WriteState<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    Configured((T, MkvRecordingConfig)),
    Recording(RecordingState<'lib, T>),
    Finished,
}

struct RecordingState<'lib, T>
where
    T: std::io::Write + std::io::Seek,
{
    mkv_segment: webm::mux::Segment<webm::mux::Writer<T>>,
    vt: webm::mux::VideoTrack,
    my_encoder: MyEncoder<'lib>,
    first_timestamp: chrono::DateTime<chrono::Utc>,
    previous_timestamp: chrono::DateTime<chrono::Utc>,
    target_interval: chrono::Duration,
}

struct NvEncoder<'lib> {
    encoder: Rc<nvenc::Encoder<'lib>>,
    vram_queue: nvenc::Queue<IOBuffer<InputBuffer<'lib>, OutputBuffer<'lib>>>,
}

pub struct IOBuffer<I, O> {
    pub in_buf: I,
    pub out_buf: O,
}
