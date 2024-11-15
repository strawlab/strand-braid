use std::{
    fs::File,
    io::{Seek, Write},
};

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};
use ci2_remote_control::FfmpegRecordingConfig;
use machine_vision_formats::{ImageStride, PixelFormat};
use mp4_writer::Mp4Writer;

// TODO: generalize also to FMF writer

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("webm writer error: {0}")]
    Mp4WriterError(#[from] mp4_writer::Error),
    #[error("SendError")]
    SendError,
    #[error(transparent)]
    RecvError(#[from] channellib::RecvError),
    #[error("already done")]
    AlreadyDone,
    #[error("disconnected")]
    Disconnected,
    #[error("filename does not end with '.mp4'")]
    FilenameDoesNotEndWithMp4,
    #[error("ffmpeg writer error {0}")]
    FfmpegWriterError(#[from] ffmpeg_writer::Error),
}

impl From<channellib::SendError<Msg>> for Error {
    #[allow(unused_variables)]
    fn from(orig: channellib::SendError<Msg>) -> Error {
        Error::SendError
    }
}

type Result<T> = std::result::Result<T, Error>;

macro_rules! async_err {
    ($rx: expr) => {
        match $rx.try_recv() {
            Ok(e) => {
                return Err(e);
            }
            Err(e) => {
                if !e.is_empty() {
                    return Err(Error::Disconnected);
                }
            }
        }
    };
}

pub struct BgMovieWriter {
    tx: channellib::Sender<Msg>,
    is_done: bool,
    err_rx: channellib::Receiver<Error>,
}

impl BgMovieWriter {
    pub fn new(
        format_str_mp4: String,
        recording_config: ci2_remote_control::RecordingConfig,
        queue_size: usize,
    ) -> Self {
        let (err_tx, err_rx) = channellib::unbounded();
        let tx = launch_runner(format_str_mp4, recording_config, queue_size, err_tx);
        Self {
            tx,
            is_done: false,
            err_rx,
        }
    }

    pub fn write<TS>(&mut self, frame: DynamicFrame, timestamp: TS) -> Result<()>
    where
        TS: Into<chrono::DateTime<chrono::Local>>,
    {
        let timestamp = timestamp.into();
        async_err!(self.err_rx);
        if self.is_done {
            return Err(Error::AlreadyDone);
        }
        let msg = Msg::Write((frame, timestamp));
        self.send(msg)
    }

    pub fn finish(&mut self) -> Result<()> {
        async_err!(self.err_rx);
        self.is_done = true;
        self.send(Msg::Finish)
    }

    fn send(&mut self, msg: Msg) -> Result<()> {
        self.tx.send(msg)?;
        Ok(())
    }
}

enum Msg {
    Write((DynamicFrame, chrono::DateTime<chrono::Local>)),
    Finish,
}

macro_rules! thread_try {
    ($tx: expr, $result: expr) => {
        match $result {
            Ok(val) => val,
            Err(e) => {
                let s = format!("send failed {}:{}: {}", file!(), line!(), e);
                $tx.send(Error::from(e)).expect(&s);
                return; // exit the thread
            }
        }
    };
}

enum RawWriter<'lib, T>
where
    T: Write + Seek,
{
    None,
    Mp4Writer(Mp4Writer<'lib, T>),
    FfmpegWriter(MyFfmpegWriter),
}

impl<'lib, T> RawWriter<'lib, T>
where
    T: Write + Seek,
{
    fn is_none(&self) -> bool {
        match self {
            Self::None => true,
            _ => false,
        }
    }
}

struct MyFfmpegWriter {
    fwtr: ffmpeg_writer::FfmpegWriter,
    swtr: srt_writer::BufferingSrtFrameWriter,
    count: usize,
}

impl MyFfmpegWriter {
    /// Save using ffmpeg to filename given.
    ///
    /// It is expected that the filename ends with '.mp4'.
    fn new(mp4_filename: &str, cfg: &FfmpegRecordingConfig) -> Result<Self> {
        if !mp4_filename.ends_with(".mp4") {
            return Err(Error::FilenameDoesNotEndWithMp4);
        }
        let mut srt_filename = mp4_filename[..mp4_filename.len() - 4].to_string();
        srt_filename.push_str(".srt");
        let args = &cfg.codec_args;
        let ffmpeg_codec_args = ffmpeg_writer::FfmpegCodecArgs {
            device_args: args.device_args.clone(),
            codec: args.codec.clone(),
            pre_codec_args: args.pre_codec_args.clone(),
            post_codec_args: args.post_codec_args.clone(),
        };
        use ci2_remote_control::RecordingFrameRate::*;
        let rate = match cfg.max_framerate {
            Fps1 => Some((1, 1)),
            Fps2 => Some((2, 1)),
            Fps5 => Some((5, 1)),
            Fps10 => Some((10, 1)),
            Fps20 => Some((20, 1)),
            Fps25 => Some((25, 1)),
            Fps30 => Some((30, 1)),
            Fps40 => Some((40, 1)),
            Fps50 => Some((50, 1)),
            Fps60 => Some((60, 1)),
            Fps100 => Some((100, 1)),
            Unlimited => None,
        };
        let fwtr = ffmpeg_writer::FfmpegWriter::new(mp4_filename, Some(ffmpeg_codec_args), rate)?;
        let out_fd = std::fs::File::create(&srt_filename)?;
        let swtr = srt_writer::BufferingSrtFrameWriter::new(Box::new(out_fd));
        Ok(Self {
            fwtr,
            swtr,
            count: 0,
        })
    }
    fn write<'a, IM, FMT>(
        &'a mut self,
        frame: &IM,
        timestamp: chrono::DateTime<chrono::Local>,
    ) -> Result<()>
    where
        IM: ImageStride<FMT>,
        FMT: PixelFormat,
    {
        let mp4_pts = self.fwtr.write_frame(frame)?;

        let msg = format!(
            "<font size=\"28\">FrameCnt: {frame_cnt}\n{timestamp}\n</font>",
            frame_cnt = self.count + 1
        );
        self.count += 1;

        self.swtr.add_frame(mp4_pts, msg)?;
        self.swtr.flush()?;
        Ok(())
    }
}

fn launch_runner<'lib>(
    format_str_mp4: String,
    recording_config: ci2_remote_control::RecordingConfig,
    size: usize,
    err_tx: channellib::Sender<Error>,
) -> channellib::Sender<Msg> {
    let (tx, rx) = channellib::bounded::<Msg>(size);
    std::thread::spawn(move || {
        // Load CUDA and nvidia-encode shared libs, but do not return error
        // (yet).
        let libs_result = nvenc::Dynlibs::new();

        let mut raw: RawWriter<'_, File> = RawWriter::None;

        let mut last_saved_stamp: Option<chrono::DateTime<chrono::Local>> = None;

        loop {
            let msg = thread_try!(err_tx, rx.recv());
            match msg {
                Msg::Write((frame, stamp)) => {
                    if raw.is_none() {
                        let local: chrono::DateTime<chrono::Local> =
                            stamp.with_timezone(&chrono::Local);
                        let mp4_filename = local.format(&format_str_mp4).to_string();

                        use ci2_remote_control::RecordingConfig::*;
                        match &recording_config {
                            Mp4(mp4_recording_config) => {
                                let mp4_path = std::path::Path::new(&mp4_filename);
                                let mp4_file = thread_try!(err_tx, std::fs::File::create(mp4_path));

                                let nv_enc = match &mp4_recording_config.codec {
                                    ci2_remote_control::Mp4Codec::H264NvEnc(_opts) => {
                                        // Now we know nvidia-encode is wanted, so
                                        // here we panic if this is not possible. In
                                        // the UI, users should not be able to choose
                                        // nvidia h264 unless CUDA devices are
                                        // available, so the panic should actually never
                                        // happen.
                                        match &libs_result {
                                            Ok(ref libs) => match nvenc::NvEnc::new(libs) {
                                                Ok(nv_enc) => Some(nv_enc),
                                                Err(e) => {
                                                    panic!(
                                                        "Error while starting \
                                                        nvidia-encode: {}",
                                                        e
                                                    );
                                                }
                                            },
                                            Err(ref e) => {
                                                panic!(
                                                    "Error while loading \
                                                CUDA or nvidia-encode: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    _ => None,
                                };

                                log::info!(
                                    "saving MP4 to {}",
                                    std::fs::canonicalize(mp4_path).unwrap().display()
                                );

                                raw = RawWriter::Mp4Writer(thread_try!(
                                    err_tx,
                                    mp4_writer::Mp4Writer::new(
                                        mp4_file,
                                        mp4_recording_config.clone(),
                                        nv_enc
                                    )
                                ));
                            }
                            Ffmpeg(c) => {
                                raw = RawWriter::FfmpegWriter(thread_try!(
                                    err_tx,
                                    MyFfmpegWriter::new(&mp4_filename, &c)
                                ));
                            }
                        };
                    }
                    let max_framerate = recording_config.max_framerate();
                    let do_save = match last_saved_stamp {
                        None => true,
                        Some(last_stamp) => {
                            let elapsed = stamp - last_stamp;
                            elapsed >= chrono::Duration::from_std(max_framerate.interval()).unwrap()
                        }
                    };
                    if do_save {
                        match &mut raw {
                            RawWriter::Mp4Writer(ref mut r) => {
                                let result = match_all_dynamic_fmts!(&frame, x, r.write(x, stamp));
                                thread_try!(err_tx, result);
                                last_saved_stamp = Some(stamp);
                            }
                            RawWriter::FfmpegWriter(ref mut r) => {
                                let result = match_all_dynamic_fmts!(&frame, x, r.write(x, stamp));
                                thread_try!(err_tx, result);
                                last_saved_stamp = Some(stamp);
                            }
                            RawWriter::None => {
                                panic!("")
                            }
                        }
                    }
                }
                Msg::Finish => {
                    match &mut raw {
                        RawWriter::Mp4Writer(ref mut mp4_writer) => {
                            thread_try!(err_tx, mp4_writer.finish());
                        }
                        RawWriter::FfmpegWriter(_) => {}
                        RawWriter::None => {
                            panic!("")
                        }
                    }
                    return; // end the thread
                }
            };
        }
    });
    tx
}
