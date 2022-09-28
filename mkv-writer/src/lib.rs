#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access, provide_any)
)]

use std::rc::Rc;

#[macro_use]
extern crate log;

use ci2_remote_control::MkvRecordingConfig;
use convert_image::convert_into;

#[cfg(feature = "vpx")]
use convert_image::{encode_y4m_frame, Y4MColorspace};

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};

use machine_vision_formats::{
    pixel_format::NV12, ImageBuffer, ImageBufferMutRef, ImageBufferRef, ImageData, ImageStride,
    PixelFormat, Stride,
};
use nvenc::{InputBuffer, OutputBuffer, RateControlMode};

use thiserror::Error;

/// Convert to runtime specified pixel format and save to FMF file.
macro_rules! convert_and_write_mkv {
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

// See https://www.fourcc.org/yuv/ and https://www.fourcc.org/rgb/
#[allow(non_camel_case_types, non_snake_case, dead_code)]
enum UncompressedFormat {
    GRAY8,
    RGB,
    BGR,
}

impl UncompressedFormat {
    fn num(&self) -> [u8; 4] {
        use UncompressedFormat::*;
        // These values are inspired by gstreamer. See
        // https://github.com/GStreamer/gst-plugins-good/commit/19a307930a9e44b5453501ec3ae8b6890ed489c0
        match self {
            GRAY8 => [b'Y', b'8', b'0', b'0'],
            RGB => [b'R', b'G', b'B', 24],
            BGR => [b'B', b'G', b'R', 24],
        }
    }
}

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
    #[cfg(feature = "vpx")]
    #[error("VPX Encoder Error")]
    VpxEncoderError {
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        inner: vpx_encode::Error,
    },
    #[error("Compiled without 'vpx' feature but VPX requested")]
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
    Uncompressed(UncompressedEncoder),
}

struct UncompressedEncoder {}

impl UncompressedEncoder {
    fn encode<'a, FRAME, FMT>(&mut self, raw_frame: &'a FRAME) -> Result<&'a [u8]>
    where
        FRAME: ImageStride<FMT>,
        FMT: PixelFormat,
    {
        let pixfmt = machine_vision_formats::pixel_format::pixfmt::<FMT>().unwrap();
        let bpp = pixfmt.bits_per_pixel();
        let row_bytes = raw_frame.width() * bpp as u32 / 8;
        if raw_frame.stride() != row_bytes as usize {
            return Err(Error::StrideError {
                #[cfg(feature = "backtrace")]
                backtrace: std::backtrace::Backtrace::capture(),
            });
        }
        let data = raw_frame.image_data();
        Ok(data)
    }
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

    pub fn write_dynamic(
        &mut self,
        frame: &DynamicFrame,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let fmt = frame.pixel_format();
        match_all_dynamic_fmts!(frame, x, convert_and_write_mkv!(fmt, self, x, timestamp));
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
                use webm::mux;

                let frame = match cfg.codec {
                    ci2_remote_control::MkvCodec::VP8(_) | ci2_remote_control::MkvCodec::VP9(_) => {
                        // The VPX encoder will error if the width is not divisible by 2.
                        let orig_width = frame.width();
                        let orig_height = frame.height();

                        let trim_width = if cfg.do_trim_size {
                            // Trim to a width which encoder can handle.
                            if orig_width % 2 != 0 {
                                // Trimming required.
                                orig_width - 1
                            } else {
                                // Trimming not required.
                                orig_width
                            }
                        } else {
                            // Use original width
                            orig_width
                        };

                        let trim_height = if cfg.do_trim_size {
                            // Trim to a height which encoder can handle.
                            if orig_height % 2 != 0 {
                                // Trimming required.
                                orig_height - 1
                            } else {
                                // Trimming not required.
                                orig_height
                            }
                        } else {
                            // Use original height
                            orig_height
                        };

                        trim_image(frame, trim_width, trim_height)
                    }
                    _ => {
                        // This path doesn't actually trim the image but still
                        // wraps the image in a TrimmedImage wrapper.
                        trim_image(frame, frame.width(), frame.height())
                    }
                };

                let width = frame.width();
                let height = frame.height();

                let mut mkv_segment =
                    mux::Segment::new(mux::Writer::new(fd)).expect("mux::Segment::new");

                let mut opt_h264_encoder = None;
                let mut opt_uncompressed_encoder = None;

                let (vpx_tup, mux_codec, mux_fourcc) = match &cfg.codec {
                    ci2_remote_control::MkvCodec::Uncompressed => {
                        use machine_vision_formats::pixel_format::*;
                        let pixfmt = pixfmt::<FMT>().unwrap();
                        let fourcc = match pixfmt {
                            PixFmt::Mono8 => UncompressedFormat::GRAY8,
                            PixFmt::RGB8 => UncompressedFormat::RGB,
                            _ => {
                                return Err(Error::UnsupportedConversion {
                                    #[cfg(feature = "backtrace")]
                                    backtrace: std::backtrace::Backtrace::capture(),
                                });
                            }
                        };
                        opt_uncompressed_encoder = Some(UncompressedEncoder {});
                        (
                            None,
                            webm::mux::VideoCodecId::Uncompressed,
                            Some(fourcc.num()),
                        )
                    }
                    #[cfg(feature = "vpx")]
                    ci2_remote_control::MkvCodec::VP8(ref opts) => (
                        Some((vpx_encode::VideoCodecId::VP8, opts.bitrate)),
                        webm::mux::VideoCodecId::VP8,
                        None,
                    ),
                    #[cfg(feature = "vpx")]
                    ci2_remote_control::MkvCodec::VP9(ref opts) => (
                        Some((vpx_encode::VideoCodecId::VP9, opts.bitrate)),
                        webm::mux::VideoCodecId::VP9,
                        None,
                    ),
                    #[cfg(not(feature = "vpx"))]
                    ci2_remote_control::MkvCodec::VP8(_) | ci2_remote_control::MkvCodec::VP9(_) => {
                        return Err(Error::NoVpxAvailable {
                            #[cfg(feature = "backtrace")]
                            backtrace: std::backtrace::Backtrace::capture(),
                        });
                    }
                    ci2_remote_control::MkvCodec::H264(ref opts) => {
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

                                opt_h264_encoder = Some(NvEncoder {
                                    encoder,
                                    vram_queue,
                                });
                                (None, webm::mux::VideoCodecId::H264, None)
                            }
                            None => return Err(Error::NvencLibsNotLoaded),
                        }
                    }
                };

                let mut vt =
                    mkv_segment.add_video_track(width, height, None, mux_codec, mux_fourcc);
                if let Some(gamma) = &cfg.gamma {
                    vt.set_gamma(*gamma);
                }

                // A dummy type which is never used so the compiler does not complain.
                #[cfg(not(feature = "vpx"))]
                #[allow(unused_variables)]
                let vpx_tup: Option<u8> = vpx_tup;

                let my_encoder = match cfg.codec {
                    ci2_remote_control::MkvCodec::Uncompressed => {
                        let enc = opt_uncompressed_encoder.unwrap();
                        MyEncoder::Uncompressed(enc)
                    }
                    ci2_remote_control::MkvCodec::H264(_) => {
                        let enc = opt_h264_encoder.unwrap();
                        MyEncoder::Nvidia(enc)
                    }
                    ci2_remote_control::MkvCodec::VP8(_) | ci2_remote_control::MkvCodec::VP9(_) => {
                        let vpx_tup = vpx_tup.unwrap();
                        #[cfg(feature = "vpx")]
                        {
                            let (vpx_codec, bitrate) = vpx_tup;
                            debug!("Using codec {:?} in mkv file.", vpx_codec);
                            // Setup the encoder.
                            let vpx_encoder = vpx_encode::Encoder::new(vpx_encode::Config {
                                width,
                                height,
                                timebase: [1, 1_000_000], // microsecond time base
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
                    }
                };

                if cfg.save_creation_time {
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
                }
                mkv_segment.set_app_name(&self.writing_application);
                if let Some(title) = &cfg.title {
                    mkv_segment.set_title(title);
                }

                // 1_000_000_000 (nanosec) / 1_000 (scale) = 1_000_000 (microseconds)
                // microseconds - timestamp in nanoseconds is divided by this scale to set PTS integer value
                mkv_segment.set_timecode_scale(1_000);

                let mut state = RecordingState {
                    mkv_segment,
                    vt,
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
                let frame = trim_image(frame, state.trim_width, state.trim_height);

                let interval = timestamp.signed_duration_since(state.previous_timestamp);
                if interval >= state.target_interval {
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

    /// Finish writing the MKV file.
    ///
    /// Calling this allows any errors to be caught explicitly. Otherwise,
    /// the MKV file will be finished when the writer is dropped. In that case,
    /// any errors will result in a panic.
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
                    MyEncoder::Uncompressed(_) => {}
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
                        nv_encoder.encoder.end_stream()?;
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

impl<'lib, T> Drop for MkvWriter<'lib, T>
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

#[cfg(feature = "vpx")]
impl<'a> PtsDur for vpx_encode::Frame<'a> {
    fn pts_dur(&self) -> std::time::Duration {
        // microsecond time base
        let secs = self.pts as f64 / 1_000_000.0;
        std::time::Duration::from_secs_f64(secs)
    }
}

fn nanos(dur: &std::time::Duration) -> u64 {
    (dur.as_secs_f64() * 1e9).round() as u64
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
        MyEncoder::Uncompressed(ref mut encoder) => {
            let timestamp_ns = nanos(&elapsed.to_std().map_err(|_| Error::TimestampTooLarge)?);
            let data = encoder.encode(raw_frame)?;
            state.vt.add_frame(data, timestamp_ns, true);
        }
        #[cfg(feature = "vpx")]
        MyEncoder::Vpx(ref mut vpx_encoder) => {
            let yuv = encode_y4m_frame(raw_frame, Y4MColorspace::C420paldv)?;
            trace!("got yuv data for frame. {} bytes.", yuv.data.len());

            let microseconds = elapsed.num_microseconds().ok_or(Error::TimestampTooLarge)?;
            for frame in vpx_encoder.encode(microseconds, &yuv.data).unwrap() {
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
                let mut dest: ImageBufferMutRef<NV12> = ImageBufferMutRef::new(dptr);
                convert_into(raw_frame, &mut dest, dest_stride)?;
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
    trim_width: u32,
    trim_height: u32,
}

struct NvEncoder<'lib> {
    encoder: Rc<nvenc::Encoder<'lib>>,
    vram_queue: nvenc::Queue<IOBuffer<InputBuffer<'lib>, OutputBuffer<'lib>>>,
}

pub struct IOBuffer<I, O> {
    pub in_buf: I,
    pub out_buf: O,
}
