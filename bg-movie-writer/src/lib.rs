use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use basic_frame::DynamicFrame;

mod movie_writer_thread;

// TODO?: generalize also to FMF writer

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("webm writer error: {0}")]
    Mp4WriterError(#[from] mp4_writer::Error),
    #[error("WorkerDisconnected")]
    WorkerDisconnected,
    #[error("AlreadyClosed")]
    AlreadyClosed,
    #[error(transparent)]
    RecvError(#[from] std::sync::mpsc::RecvError),
    #[error("already done")]
    AlreadyDone,
    #[error("disconnected")]
    Disconnected,
    #[error("filename does not end with '.mp4'")]
    FilenameDoesNotEndWithMp4,
    #[error("ffmpeg writer error {0}")]
    FfmpegWriterError(#[from] ffmpeg_writer::Error),
}

type Result<T> = std::result::Result<T, Error>;

/// From outside the worker thread, check if we received an error from the
/// thread.
macro_rules! poll_err {
    ($err_rx: expr) => {{
        if let Some(e) = $err_rx.lock().unwrap().take() {
            return Err(e);
        }
    }};
}

pub struct BgMovieWriter {
    tx: std::sync::mpsc::SyncSender<Msg>,
    is_done: bool,
    err_from_worker: Arc<Mutex<Option<Error>>>,
}

impl BgMovieWriter {
    /// This spawns a writer thread which will save a movie in the background.
    /// The methods [Self::write] and [Self::finish] return immediately, even
    /// though their work is not done.
    ///
    /// - `format_str_mp4` determines the filename used after formatting with
    /// [chrono::DateTime::format].
    /// - `queue_size` is the number of frames that can be buffered before
    /// frames will be dropped.
    pub fn new(
        format_str_mp4: String,
        recording_config: ci2_remote_control::RecordingConfig,
        queue_size: usize,
        data_dir: Option<PathBuf>,
    ) -> Self {
        // Create an Arc<Mutex<Option<Error>>> to hold a potential error from
        // the to-be-spawned writer thread.
        let err_to_launcher = Arc::new(Mutex::new(None));
        let err_from_worker = err_to_launcher.clone();
        // Create a channel to send data into the writer thread.
        let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(queue_size);
        // Spawn the writer thread
        std::thread::spawn(move || {
            // Runs until the movie is done.
            movie_writer_thread::writer_thread_loop(
                format_str_mp4,
                recording_config,
                err_to_launcher,
                data_dir,
                rx,
            )
        });
        Self {
            tx,
            is_done: false,
            err_from_worker,
        }
    }

    /// Enqueue the frame and timestamp for writing to the background thread.
    ///
    /// If the background writer thread has previously encountered an error,
    /// this will return that previously-encountered error.
    pub fn write<TS>(&mut self, frame: DynamicFrame, timestamp: TS) -> Result<()>
    where
        TS: Into<chrono::DateTime<chrono::Local>>,
    {
        let timestamp = timestamp.into();
        poll_err!(self.err_from_worker);
        if self.is_done {
            return Err(Error::AlreadyDone);
        }
        let msg = Msg::Write((frame, timestamp));
        // This will only succeed if the channel is not full. It will not block.
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(std::sync::mpsc::TrySendError::Full(_msg)) => {
                tracing::warn!("Dropping frame to save: channel full");
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_msg)) => {
                return Err(Error::WorkerDisconnected);
            }
        }
        Ok(())
    }

    /// Enqueue a message telling the background thread to finish writing.
    ///
    /// If the background writer thread has previously encountered an error,
    /// this will return that previously-encountered error.
    pub fn finish(&mut self) -> Result<()> {
        poll_err!(self.err_from_worker);
        self.is_done = true;
        let tx = self.tx.clone();
        // We want to send the finish message without fail, so spawn a new
        // thread which blocks until the message can be sent. If we don't spawn
        // a new thread, the writer thread could be busy and block. If we don't
        // block on sending, a full channel could cause the finish message to be
        // dropped.
        std::thread::spawn(move || {
            // If the receiver has disconnected, this will panic.
            tx.send(Msg::Finish).unwrap();
        });
        Ok(())
    }
}

pub(crate) enum Msg {
    Write((DynamicFrame, chrono::DateTime<chrono::Local>)),
    Finish,
}
