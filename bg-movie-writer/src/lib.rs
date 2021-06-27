#![cfg_attr(feature = "backtrace", feature(backtrace))]

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use basic_frame::{match_all_dynamic_fmts, DynamicFrame};

// TODO: generalize also to FMF writer

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("webm writer error: {0}")]
    MkvWriterError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        mkv_writer::Error,
    ),
    #[error("mkvfix error: {0}")]
    MkvFix(#[from] strand_cam_mkvfix::Error),
    #[error("SendError")]
    SendError(#[cfg(feature = "backtrace")] Backtrace),
    #[error(transparent)]
    RecvError(
        #[from]
        #[cfg_attr(feature = "backtrace", backtrace)]
        channellib::RecvError,
    ),
    #[error("already done")]
    AlreadyDone(#[cfg(feature = "backtrace")] Backtrace),
    #[error("disconnected")]
    Disconnected(#[cfg(feature = "backtrace")] Backtrace),
}

impl From<channellib::SendError<Msg>> for Error {
    fn from(orig: channellib::SendError<Msg>) -> Error {
        Error::SendError(
            #[cfg(feature = "backtrace")]
            orig.backtrace,
        )
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
                    return Err(Error::Disconnected(
                        #[cfg(feature = "backtrace")]
                        Backtrace::capture(),
                    ));
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
    pub fn new_webm_writer(
        format_str_mkv: String,
        mkv_recording_config: ci2_remote_control::MkvRecordingConfig,
        queue_size: usize,
    ) -> Self {
        let (err_tx, err_rx) = channellib::unbounded();
        let tx = launch_runner(format_str_mkv, mkv_recording_config, queue_size, err_tx);
        Self {
            tx,
            is_done: false,
            err_rx,
        }
    }

    pub fn write(
        &mut self,
        frame: DynamicFrame,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        async_err!(self.err_rx);
        if self.is_done {
            return Err(Error::AlreadyDone(
                #[cfg(feature = "backtrace")]
                Backtrace::capture(),
            ));
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
    Write((DynamicFrame, chrono::DateTime<chrono::Utc>)),
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

fn launch_runner(
    format_str_mkv: String,
    mkv_recording_config: ci2_remote_control::MkvRecordingConfig,
    size: usize,
    err_tx: channellib::Sender<Error>,
) -> channellib::Sender<Msg> {
    let (tx, rx) = channellib::bounded::<Msg>(size);
    std::thread::spawn(move || {
        // Load CUDA and nvidia-encode shared libs, but do not return error
        // (yet).
        let libs_result = nvenc::Dynlibs::new();

        let mut raw = None;

        loop {
            let msg = thread_try!(err_tx, rx.recv());
            match msg {
                Msg::Write((frame, stamp)) => {
                    if raw.is_none() {
                        let local: chrono::DateTime<chrono::Local> =
                            stamp.with_timezone(&chrono::Local);
                        let filename = local.format(&format_str_mkv).to_string();
                        let path = std::path::Path::new(&filename);
                        let mut h264_path = None;
                        let f = thread_try!(err_tx, std::fs::File::create(&path));

                        let nv_enc = match &mkv_recording_config.codec {
                            ci2_remote_control::MkvCodec::H264(_opts) => {
                                h264_path = Some(std::path::PathBuf::from(path));
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

                        raw = Some((
                            h264_path,
                            thread_try!(
                                err_tx,
                                mkv_writer::MkvWriter::new(f, mkv_recording_config.clone(), nv_enc)
                            ),
                        ));
                    }
                    if let Some((_h264_path, ref mut r)) = &mut raw {
                        let result = match_all_dynamic_fmts!(&frame, x, r.write(x, stamp));
                        thread_try!(err_tx, result);
                    }
                }
                Msg::Finish => {
                    if raw.is_some() {
                        let (h264_path, mut mkv_writer) = raw.unwrap();
                        thread_try!(err_tx, mkv_writer.finish());

                        if let Some(path) = h264_path {
                            if strand_cam_mkvfix::is_ffmpeg_available() {
                                thread_try!(err_tx, strand_cam_mkvfix::mkv_fix(path));
                            } else {
                                log::error!(
                                    "Could not fix file {} because `ffmpeg` program not available. \
                                The file may not display timestamps or seek correctly in some players.",
                                    path.display()
                                );
                            }
                        }
                    }
                    return; // end the thread
                }
            };
        }
    });
    tx
}
