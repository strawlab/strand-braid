use crossbeam_channel::TryRecvError;

use machine_vision_formats::ImageStride;

// TODO: generalize also to FMF writer

#[derive(Debug)]
pub enum ErrorKind {
    IoError(std::io::Error),
    WebmWriterError(webm_writer::Error),
    MkvFix(strand_cam_mkvfix::Error),
    SendError,
    RecvError,
    AlreadyDone,
    Disconnected,
}

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        Self { kind }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}

impl From<std::io::Error> for Error {
    fn from(orig: std::io::Error) -> Error {
        Error {
            kind: ErrorKind::IoError(orig),
        }
    }
}

impl From<webm_writer::Error> for Error {
    fn from(orig: webm_writer::Error) -> Error {
        Error {
            kind: ErrorKind::WebmWriterError(orig),
        }
    }
}

impl From<strand_cam_mkvfix::Error> for Error {
    fn from(orig: strand_cam_mkvfix::Error) -> Error {
        Error {
            kind: ErrorKind::MkvFix(orig),
        }
    }
}

impl<IM> From<crossbeam_channel::SendError<Msg<IM>>> for Error
where
    IM: ImageStride + Send,
{
    fn from(_orig: crossbeam_channel::SendError<Msg<IM>>) -> Error {
        Error {
            kind: ErrorKind::SendError,
        }
    }
}

impl From<crossbeam_channel::RecvError> for Error {
    fn from(_orig: crossbeam_channel::RecvError) -> Error {
        Error {
            kind: ErrorKind::RecvError,
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

macro_rules! async_err {
    ($rx: expr) => {
        match $rx.try_recv() {
            Ok(e) => {
                return Err(e);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Err(Error::new(ErrorKind::Disconnected));
            }
        }
    };
}

pub struct BgMovieWriter<IM>
where
    IM: ImageStride,
{
    tx: crossbeam_channel::Sender<Msg<IM>>,
    is_done: bool,
    err_rx: crossbeam_channel::Receiver<Error>,
}

impl<IM> BgMovieWriter<IM>
where
    IM: 'static + ImageStride + Send,
{
    pub fn new_webm_writer(
        format_str_mkv: String,
        mkv_recording_config: ci2_remote_control::MkvRecordingConfig,
        queue_size: usize,
    ) -> Self {
        let (err_tx, err_rx) = crossbeam_channel::unbounded();
        let tx = launch_runner(format_str_mkv, mkv_recording_config, queue_size, err_tx);
        Self {
            tx,
            is_done: false,
            err_rx,
        }
    }

    pub fn write(&mut self, frame: IM, timestamp: chrono::DateTime<chrono::Utc>) -> Result<()> {
        async_err!(self.err_rx);
        if self.is_done {
            return Err(Error::new(ErrorKind::AlreadyDone));
        }
        let msg = Msg::Write((frame, timestamp));
        self.send(msg)
    }

    pub fn finish(&mut self) -> Result<()> {
        async_err!(self.err_rx);
        self.is_done = true;
        self.send(Msg::Finish)
    }

    fn send(&mut self, msg: Msg<IM>) -> Result<()> {
        self.tx.send(msg)?;
        Ok(())
    }
}

enum Msg<IM>
where
    IM: ImageStride,
{
    Write((IM, chrono::DateTime<chrono::Utc>)),
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

fn launch_runner<IM>(
    format_str_mkv: String,
    mkv_recording_config: ci2_remote_control::MkvRecordingConfig,
    size: usize,
    err_tx: crossbeam_channel::Sender<Error>,
) -> crossbeam_channel::Sender<Msg<IM>>
where
    IM: 'static + ImageStride + Send,
{
    let (tx, rx) = crossbeam_channel::bounded::<Msg<IM>>(size);
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
                                webm_writer::WebmWriter::new(
                                    f,
                                    mkv_recording_config.clone(),
                                    nv_enc
                                )
                            ),
                        ));
                    }
                    if let Some((_h264_path, ref mut r)) = &mut raw {
                        thread_try!(err_tx, r.write(&frame, stamp));
                    }
                }
                Msg::Finish => {
                    if raw.is_some() {
                        let (h264_path, mut webm_writer) = raw.unwrap();
                        thread_try!(err_tx, webm_writer.finish());

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
