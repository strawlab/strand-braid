// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::VecDeque,
    io::Write,
    process::{Child, ChildStdin, Command, Stdio},
};

use machine_vision_formats::pixel_format::PixFmt;

const FFMPEG: &str = "ffmpeg";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ffmpeg error ({})", output.status)]
    FfmpegError { output: std::process::Output },
    #[error("string not valid UTF8")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
    #[error("unexpected ffmpeg output: {0}")]
    UnexpectedFfmpegOutput(String),
    #[error("the frame format or size changed mid-stream")]
    FormatOrSizeChanged,
    // We deliberately do not (yet) convert unsupported pixel formats to a format
    // ffmpeg accepts; such conversions belong in the `convert-image` crate. For
    // now, formats without a direct raw-video equivalent are unimplemented.
    #[error("no direct ffmpeg raw-video pixel format for {0}; conversion unimplemented")]
    UnimplementedPixelFormat(PixFmt),
}

type Result<T> = std::result::Result<T, Error>;

/// The ffmpeg raw-video (`-f rawvideo`) pixel-format name corresponding to a
/// [`PixFmt`], if the bytes can be piped to ffmpeg without any conversion on
/// our side (ffmpeg itself does any conversion the encoder needs).
///
/// Returns `Err(Error::UnimplementedPixelFormat)` for formats that would
/// require us to convert first (e.g. 32-bit float or planar formats).
pub fn ffmpeg_pixel_format(pixfmt: PixFmt) -> Result<&'static str> {
    use PixFmt::*;
    // The Bayer names differ between the machine-vision-formats convention
    // (named by the first two pixels of the first row) and ffmpeg's (named by
    // the top-left 2x2 block): e.g. `BayerRG8` (row0 = R,G; row1 = G,B) is
    // ffmpeg's `bayer_rggb8`.
    Ok(match pixfmt {
        Mono8 => "gray",
        RGB8 => "rgb24",
        // machine-vision-formats YUV422 is UYVY-packed ([U, Y0, V, Y1]).
        YUV422 => "uyvy422",
        BayerRG8 => "bayer_rggb8",
        BayerGR8 => "bayer_grbg8",
        BayerGB8 => "bayer_gbrg8",
        BayerBG8 => "bayer_bggr8",
        other => return Err(Error::UnimplementedPixelFormat(other)),
    })
}

/// Saves video frames to a video file using ffmpeg.
///
/// This spawns an ffmpeg process and pipes the frames as raw video
/// (`-f rawvideo`) with no intermediate format conversion on our side; ffmpeg
/// performs whatever conversion the chosen encoder requires. The ffmpeg process
/// is spawned lazily on the first frame, once the frame width, height and pixel
/// format are known.
pub struct FfmpegWriter {
    fname: String,
    ffmpeg_codec_args: FfmpegCodecArgs,
    raten: usize,
    rated: usize,
    count: usize,
    running: Option<Running>,
}

/// State of the spawned ffmpeg process, created on the first frame.
struct Running {
    child: Child,
    stdin: ChildStdin,
    pixfmt: PixFmt,
    width: u32,
    height: u32,
}

type FfmpegCodecArgList = Option<Vec<(String, String)>>;

#[derive(Debug, PartialEq, Clone, Default)]
pub struct FfmpegCodecArgs {
    pub device_args: FfmpegCodecArgList,
    pub pre_codec_args: FfmpegCodecArgList,
    pub codec: Option<String>,
    pub post_codec_args: FfmpegCodecArgList,
}

fn prefix() -> Vec<String> {
    zq(&["-nostats", "-hide_banner", "-nostdin", "-y"])
}

fn zq(x: &[&str]) -> Vec<String> {
    x.iter().map(|x| (*x).into()).collect()
}

fn zq2(opt_x: Option<&Vec<(String, String)>>) -> Vec<String> {
    if let Some(x) = opt_x {
        x.iter()
            .flat_map(|(x1, x2)| [x1.clone(), x2.clone()])
            .collect()
    } else {
        vec![]
    }
}

impl FfmpegCodecArgs {
    /// Build the full ffmpeg argument list.
    ///
    /// `input_args` are inserted immediately before `-i -` and describe the raw
    /// video arriving on stdin (format, pixel format, size, frame rate, color
    /// range). We also force full-range (`pc`) output so the full 0-255
    /// intensity range is preserved rather than the limited "tv" range.
    fn to_args(&self, input_args: &[String]) -> Vec<String> {
        const VIDEO_CODEC: &str = "-c:v";
        let output_color_range = zq(&["-color_range", "pc"]);
        let input: Vec<String> = input_args.to_vec();
        let stdin_input = zq(&["-i", "-"]);
        if let Some(codec) = &self.codec {
            vec![
                prefix(),
                zq2(self.device_args.as_ref()),
                input,
                stdin_input,
                zq2(self.pre_codec_args.as_ref()),
                zq(&[VIDEO_CODEC, codec]),
                zq2(self.post_codec_args.as_ref()),
                output_color_range,
            ]
        } else {
            assert_eq!(self.device_args, None);
            assert_eq!(self.pre_codec_args, None);
            assert_eq!(self.post_codec_args, None);
            vec![prefix(), input, stdin_input, output_color_range]
        }
        .into_iter()
        .flatten()
        .collect()
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            // Keep these in sync with the list in strand-cam-remote-control.
            "vaapi" => Some(Self {
                device_args: Some(vec![("-vaapi_device".into(), "/dev/dri/renderD128".into())]),
                pre_codec_args: Some(vec![("-vf".into(), "format=nv12,hwupload".into())]),
                codec: Some("h264_vaapi".to_string()),
                post_codec_args: Some(vec![("-color_range".into(), "pc".into())]),
            }),
            "videotoolbox" => Some(Self {
                codec: Some("h264_videotoolbox".into()),
                ..Default::default()
            }),
            _ => None,
        }
    }
}

pub fn ffmpeg_version() -> Result<String> {
    let args = ["-hide_banner", "-nostdin", "-version"];
    let ffmpeg_child = Command::new(FFMPEG)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out = ffmpeg_child.wait_with_output()?;
    let lines = String::from_utf8(out.stdout)?;

    let mut ffmpeg_stderr_iter = lines.split_ascii_whitespace();
    assert_eq!(ffmpeg_stderr_iter.next(), Some("ffmpeg"));
    assert_eq!(ffmpeg_stderr_iter.next(), Some("version"));

    if let Some(version_str) = ffmpeg_stderr_iter.next() {
        Ok(version_str.into())
    } else {
        Err(Error::UnexpectedFfmpegOutput(lines))
    }
}

pub fn platform_hardware_encoder() -> Result<FfmpegCodecArgs> {
    let args = ["-hide_banner", "-nostdin", "-hwaccels"];
    let ffmpeg_child = Command::new(FFMPEG)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out = ffmpeg_child.wait_with_output()?;
    let lines = String::from_utf8(out.stdout)?;
    let mut lines: VecDeque<_> = lines.split("\n").collect();
    let line0 = lines.pop_front().unwrap();
    if line0 != "Hardware acceleration methods:" {
        return Err(Error::UnexpectedFfmpegOutput(line0.into()));
    }
    for line in lines.into_iter() {
        if let Some(opt) = FfmpegCodecArgs::from_str(line) {
            return Ok(opt);
        }
    }
    Ok(FfmpegCodecArgs {
        ..Default::default()
    })
}

impl FfmpegWriter {
    pub fn new(
        fname: &str,
        ffmpeg_codec_args: FfmpegCodecArgs,
        rate: Option<(usize, usize)>,
    ) -> Result<Self> {
        let (raten, rated) = rate.unwrap_or((25, 1));
        // ffmpeg is spawned lazily on the first frame, once we know the frame
        // width, height and pixel format needed for the raw-video input options.
        Ok(Self {
            fname: fname.to_string(),
            ffmpeg_codec_args,
            raten,
            rated,
            count: 0,
            running: None,
        })
    }

    /// Spawn ffmpeg configured to read raw video of this frame's format.
    fn start(&mut self, frame: &strand_dynamic_frame::DynamicFrame) -> Result<()> {
        let pixfmt = frame.pixel_format();
        let ff_pixfmt = ffmpeg_pixel_format(pixfmt)?;
        let width = frame.width();
        let height = frame.height();

        // Raw-video input options, placed just before `-i -`. We tag the input
        // as full range (`pc`) so ffmpeg preserves the full 0-255 range.
        let input_args = vec![
            "-f".to_string(),
            "rawvideo".to_string(),
            "-pixel_format".to_string(),
            ff_pixfmt.to_string(),
            "-video_size".to_string(),
            format!("{width}x{height}"),
            "-framerate".to_string(),
            format!("{}/{}", self.raten, self.rated),
            "-color_range".to_string(),
            "pc".to_string(),
        ];

        let mut args = self.ffmpeg_codec_args.to_args(&input_args);
        args.push(self.fname.clone());

        let show_ffmpeg = match std::env::var_os("FFMPEG_WRITER_SHOW") {
            Some(v) => &v != "0",
            None => false,
        };
        if show_ffmpeg {
            println!("ffmpeg {}", args.join(" "));
        }

        let mut cmd0 = Command::new(FFMPEG);
        let cmd = cmd0.args(args).stdin(Stdio::piped());
        let cmd = if show_ffmpeg {
            cmd
        } else {
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped())
        };
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("failed to get stdin");

        self.running = Some(Running {
            child,
            stdin,
            pixfmt,
            width,
            height,
        });
        Ok(())
    }

    /// Write a frame. Return the presentation timestamp (PTS).
    pub fn write_dynamic_frame(
        &mut self,
        frame: &strand_dynamic_frame::DynamicFrame,
    ) -> Result<std::time::Duration> {
        if self.running.is_none() {
            self.start(frame)?;
        }
        let running = self.running.as_mut().unwrap();
        if frame.pixel_format() != running.pixfmt
            || frame.width() != running.width
            || frame.height() != running.height
        {
            return Err(Error::FormatOrSizeChanged);
        }

        // Pipe the raw frame data row by row (stripping any stride padding).
        let stdin = &mut running.stdin;
        let io_result: std::io::Result<()> = strand_dynamic_frame::match_all_dynamic_fmts!(
            frame,
            x,
            {
                use machine_vision_formats::iter::HasRowChunksExact;
                let mut res = Ok(());
                for row in x.rowchunks_exact() {
                    if let Err(e) = stdin.write_all(row) {
                        res = Err(e);
                        break;
                    }
                }
                res
            },
            // Reached only for formats start() did not already reject.
            Error::UnimplementedPixelFormat(frame.pixel_format())
        );

        match io_result {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                // ffmpeg apparently died; surface its output as the error.
                return Err(self.collect_ffmpeg_error());
            }
            Err(e) => return Err(e.into()),
        }

        let num = self.rated * self.count;
        let dur_sec = num as f64 / self.raten as f64;
        let pts = std::time::Duration::from_secs_f64(dur_sec);
        self.count += 1;
        Ok(pts)
    }

    /// Wait for the (apparently dead) ffmpeg process and collect its output.
    fn collect_ffmpeg_error(&mut self) -> Error {
        let mut running = self.running.take().unwrap();
        let status = match running.child.wait() {
            Ok(status) => status,
            Err(e) => return Error::Io(e),
        };
        use std::io::Read;
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        if let Some(mut out) = running.child.stdout.take() {
            let _ = out.read_to_end(&mut stdout);
        }
        if let Some(mut err) = running.child.stderr.take() {
            let _ = err.read_to_end(&mut stderr);
        }
        Error::FfmpegError {
            output: std::process::Output {
                status,
                stdout,
                stderr,
            },
        }
    }

    pub fn close(self) -> Result<()> {
        // Close ffmpeg's stdin (telling it to finish) by dropping it, then wait.
        let Some(running) = self.running else {
            // No frames were ever written, so ffmpeg was never spawned.
            return Ok(());
        };
        let Running { child, stdin, .. } = running;
        std::mem::drop(stdin);
        let output = child.wait_with_output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::FfmpegError { output })
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use machine_vision_formats::PixFmt;
    use strand_dynamic_frame::DynamicFrameOwned;

    /// One pixel format's lossless round-trip case.
    struct Case {
        pixfmt: PixFmt,
        /// The ffmpeg raw-video pixel-format name the mapping should produce.
        /// Hardcoded (not read from the code under test) so it is an independent
        /// ground truth: the writer encodes using `ffmpeg_pixel_format(pixfmt)`,
        /// while we decode back using this. If the mapping were wrong, encode
        /// and decode would disagree and the round trip would not match.
        ffmpeg_pixfmt: &'static str,
        /// Bytes per pixel of the packed layout (used to compute row size).
        bytes_per_pixel: usize,
        /// A lossless codec that preserves this format's bytes exactly.
        codec: &'static str,
        /// Container extension matching `codec`.
        ext: &'static str,
    }

    /// Decode the first (only) video frame of `path` back to tightly packed raw
    /// bytes in `pix_fmt`, via ffmpeg.
    fn ffmpeg_decode_raw(path: &std::path::Path, pix_fmt: &str) -> Vec<u8> {
        let output = std::process::Command::new(FFMPEG)
            .args(["-nostdin", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", pix_fmt, "-"])
            .output()
            .expect("running ffmpeg to decode the recording");
        assert!(
            output.status.success(),
            "ffmpeg decode failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn assert_roundtrips_exactly(case: &Case) {
        // Direct check of the mapping under test against the ground truth.
        assert_eq!(
            ffmpeg_pixel_format(case.pixfmt).unwrap(),
            case.ffmpeg_pixfmt,
            "unexpected ffmpeg pixel-format mapping for {:?}",
            case.pixfmt
        );
        let ffmpeg_pixfmt = case.ffmpeg_pixfmt;
        let (width, height) = (64u32, 48u32);
        let valid_stride = width as usize * case.bytes_per_pixel;
        // Give the frame stride padding so we also exercise the writer stripping
        // it off before piping (rows must arrive tightly packed).
        let pad = 16usize;
        let stride = valid_stride + pad;

        // Deterministic, non-constant content so any misframing or channel-order
        // mistake in the pixel-format mapping would change the decoded bytes.
        let mut buf = vec![0xAAu8; height as usize * stride]; // padding sentinel
        let mut expected = Vec::with_capacity(height as usize * valid_stride);
        for row in 0..height as usize {
            for i in 0..valid_stride {
                let v = ((row * valid_stride + i) * 31 + 7) as u8;
                buf[row * stride + i] = v;
                expected.push(v);
            }
        }

        let frame = DynamicFrameOwned::from_buf(width, height, stride, buf, case.pixfmt).unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let out_path = tmp.path().join(format!("roundtrip.{}", case.ext));
        {
            let codec_args = FfmpegCodecArgs {
                codec: Some(case.codec.to_string()),
                ..Default::default()
            };
            let mut wtr = FfmpegWriter::new(out_path.to_str().unwrap(), codec_args, None).unwrap();
            wtr.write_dynamic_frame(&frame.borrow()).unwrap();
            wtr.close().unwrap();
        }

        let got = ffmpeg_decode_raw(&out_path, ffmpeg_pixfmt);
        assert_eq!(
            got.len(),
            expected.len(),
            "{:?} ({ffmpeg_pixfmt}): decoded byte count differs",
            case.pixfmt
        );
        assert!(
            got == expected,
            "{:?} ({ffmpeg_pixfmt}): pixel data did not round-trip exactly through ffmpeg",
            case.pixfmt
        );
    }

    /// Frame data piped raw to ffmpeg (see the crate docs / `ffmpeg_pixel_format`)
    /// must survive a round trip byte-for-byte. Mono8/RGB8/YUV422 go through the
    /// lossless FFV1 codec, which also interprets the colorspace and so catches
    /// channel-order mistakes (e.g. RGB vs BGR, UYVY vs YUYV). Bayer has no
    /// non-debayering codec, so it uses the verbatim `rawvideo` codec; that the
    /// real (H.264) encoder accepts each format is covered by the sim smoke test.
    #[test]
    fn frame_data_roundtrips_exactly_via_ffmpeg() {
        let cases = [
            Case {
                pixfmt: PixFmt::Mono8,
                ffmpeg_pixfmt: "gray",
                bytes_per_pixel: 1,
                codec: "ffv1",
                ext: "mkv",
            },
            Case {
                pixfmt: PixFmt::RGB8,
                ffmpeg_pixfmt: "rgb24",
                bytes_per_pixel: 3,
                codec: "ffv1",
                ext: "mkv",
            },
            Case {
                pixfmt: PixFmt::YUV422,
                ffmpeg_pixfmt: "uyvy422",
                bytes_per_pixel: 2,
                codec: "ffv1",
                ext: "mkv",
            },
            Case {
                pixfmt: PixFmt::BayerRG8,
                ffmpeg_pixfmt: "bayer_rggb8",
                bytes_per_pixel: 1,
                codec: "rawvideo",
                ext: "nut",
            },
        ];
        for case in &cases {
            assert_roundtrips_exactly(case);
        }
    }
}
