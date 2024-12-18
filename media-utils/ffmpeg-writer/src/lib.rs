use machine_vision_formats as formats;
use std::{
    collections::VecDeque,
    process::{Child, Command, Stdio},
};

const FFMPEG: &str = "ffmpeg";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("y4m_writer error: {0}")]
    Y4mWriter(#[from] y4m_writer::Error),
    #[error("ffmpeg error ({})", output.status)]
    FfmpegError { output: std::process::Output },
    #[error("string not valid UTF8")]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
    #[error("unexpected ffmpeg output: {0}")]
    UnexpectedFfmpegOutput(String),
}

type Result<T> = std::result::Result<T, Error>;

/// Saves video frames to a video file using ffmpeg.
///
/// This spawns an ffmpeg process and pipes the frames as y4m.
pub struct FfmpegWriter {
    wtr: y4m_writer::Y4MWriter,
    ffmpeg_child: Option<Child>,
    count: usize,
    raten: usize,
    rated: usize,
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

fn middle() -> Vec<String> {
    zq(&["-i", "-", "-fps_mode", "passthrough"])
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
    fn to_args(&self) -> Vec<String> {
        const VIDEO_CODEC: &str = "-c:v";
        {
            {
                if let Some(codec) = &self.codec {
                    vec![
                        prefix(),
                        zq2(self.device_args.as_ref()),
                        middle(),
                        zq2(self.pre_codec_args.as_ref()),
                        zq(&[VIDEO_CODEC, codec]),
                        zq2(self.post_codec_args.as_ref()),
                    ]
                } else {
                    assert_eq!(self.device_args, None);
                    assert_eq!(self.pre_codec_args, None);
                    assert_eq!(self.post_codec_args, None);
                    vec![prefix(), middle()]
                }
            }
        }
        .into_iter()
        .flatten()
        .collect()
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "vaapi" => Some(Self {
                device_args: Some(vec![("-vaapi_device".into(), "/dev/dri/renderD128".into())]),
                pre_codec_args: Some(vec![("-vf".into(), "format=nv12,hwupload".into())]),
                codec: Some("h264_vaapi".to_string()),
                ..Default::default()
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
        ffmpeg_codec_args: Option<FfmpegCodecArgs>,
        rate: Option<(usize, usize)>,
    ) -> Result<Self> {
        let (raten, rated) = rate.unwrap_or((25, 1));
        let y4m_opts = y4m_writer::Y4MOptions {
            raten,
            rated,
            aspectn: 1,
            aspectd: 1,
        };
        let (wtr, ffmpeg_child) = if let Some(ffmpeg_codec_args) = ffmpeg_codec_args {
            let mut args = ffmpeg_codec_args.to_args();
            args.push(fname.into());
            let mut ffmpeg_child = Command::new(FFMPEG)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let stdin = ffmpeg_child.stdin.take().expect("failed to get stdin");

            let wtr = y4m_writer::Y4MWriter::from_writer(Box::new(stdin), y4m_opts);
            (wtr, Some(ffmpeg_child))
        } else {
            // No ffmpeg, just raw y4m file
            let fd = std::fs::File::create(fname)?;
            let wtr = y4m_writer::Y4MWriter::from_writer(Box::new(fd), y4m_opts);
            (wtr, None)
        };

        Ok(Self {
            wtr,
            ffmpeg_child,
            count: 0,
            raten,
            rated,
        })
    }

    /// Write a frame. Return the presentation timestamp (PTS).
    pub fn write_frame<F>(
        &mut self,
        frame: &dyn formats::iter::HasRowChunksExact<F>,
    ) -> Result<std::time::Duration>
    where
        F: formats::pixel_format::PixelFormat,
    {
        match self.wtr.write_frame(frame) {
            Ok(()) => {}
            Err(y4m_writer::Error::Y4mError(y4m::Error::IoError(e)))
                if e.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                if let Some(ffmpeg_child) = &mut self.ffmpeg_child {
                    // Apparently ffmpeg died.
                    //
                    // Should we call `self.ffmpeg_child.kill()` or assume ffmpeg
                    // died?
                    let status = ffmpeg_child.wait()?;
                    use std::io::Read;
                    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
                    let mut out = ffmpeg_child.stdout.take().unwrap();
                    let mut err = ffmpeg_child.stderr.take().unwrap();
                    out.read_to_end(&mut stdout)?;
                    err.read_to_end(&mut stderr)?;
                    let output = std::process::Output {
                        status,
                        stdout,
                        stderr,
                    };
                    return Err(Error::FfmpegError { output });
                } else {
                    todo!();
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        }
        self.wtr.flush()?;
        let num = self.rated * self.count;
        let dur_sec = num as f64 / self.raten as f64;
        let pts = std::time::Duration::from_secs_f64(dur_sec);
        self.count += 1;
        Ok(pts)
    }

    pub fn close(self) -> Result<()> {
        // Close the writer, telling ffmpeg also to end.
        let wtr = self.wtr.into_inner();
        std::mem::drop(wtr);

        if let Some(ffmpeg_child) = self.ffmpeg_child {
            // Wait for ffmpeg to end.
            let output = ffmpeg_child.wait_with_output()?;
            if output.status.success() {
                Ok(())
            } else {
                Err(Error::FfmpegError { output })
            }
        } else {
            Ok(())
        }
    }
}
