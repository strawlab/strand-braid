use machine_vision_formats as formats;
use std::{
    collections::VecDeque,
    process::{Child, Command, Stdio},
};

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

pub struct FfmpegWriter {
    wtr: y4m_writer::Y4MWriter,
    ffmpeg_child: Option<Child>,
}

#[derive(Debug, PartialEq)]
pub enum FfmpegEncoderOptions {
    H264VideoToolbox,
    H264Nvenc,
    H264Vaapi,
    BareFfmpeg,
}

fn prefix() -> Vec<String> {
    zq(&["-hide_banner", "-nostdin", "-y"])
}

fn middle() -> Vec<String> {
    zq(&["-i", "-", "-fps_mode", "passthrough"])
}

fn zq(x: &[&str]) -> Vec<String> {
    x.into_iter().map(|x| String::from(*x)).collect()
}

impl FfmpegEncoderOptions {
    fn to_args(&self) -> Vec<String> {
        const VIDEO_CODEC: &str = "-c:v";
        use FfmpegEncoderOptions::*;
        match &self {
            H264Vaapi => vec![
                prefix(),
                zq(&["-vaapi_device", "/dev/dri/renderD128"]),
                middle(),
                zq(&["-vf", "format=nv12,hwupload", VIDEO_CODEC, "h264_vaapi"]),
            ],
            H264Nvenc => vec![prefix(), middle(), zq(&[VIDEO_CODEC, "h264_nvenc"])],
            H264VideoToolbox => vec![prefix(), middle(), zq(&[VIDEO_CODEC, "h264_videotoolbox"])],
            BareFfmpeg => vec![prefix(), middle()],
        }
        .into_iter()
        .flatten()
        .collect()
    }

    fn from_str(s: &str) -> Option<Self> {
        use FfmpegEncoderOptions::*;
        match s {
            "vaapi" => Some(H264Vaapi),
            "videotoolbox" => Some(H264VideoToolbox),
            _ => None,
        }
    }
}

pub fn platform_hardware_encoder() -> Result<FfmpegEncoderOptions> {
    let args = ["-hide_banner", "-nostdin", "-hwaccels"];
    let ffmpeg_child = Command::new("ffmpeg")
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
        if let Some(opt) = FfmpegEncoderOptions::from_str(line) {
            return Ok(opt);
        }
    }
    Ok(FfmpegEncoderOptions::BareFfmpeg)
}

impl FfmpegWriter {
    pub fn new(fname: &str, opts: Option<FfmpegEncoderOptions>) -> Result<Self> {
        let y4m_opts = y4m_writer::Y4MOptions {
            raten: 25,
            rated: 1,
            aspectn: 1,
            aspectd: 1,
        };
        let (wtr, ffmpeg_child) = if let Some(opts) = opts {
            let mut args = opts.to_args();
            args.push(fname.into());
            let mut ffmpeg_child = Command::new("ffmpeg")
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

        Ok(Self { wtr, ffmpeg_child })
    }

    pub fn write_frame<F>(&mut self, frame: &dyn formats::iter::HasRowChunksExact<F>) -> Result<()>
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
        Ok(())
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
