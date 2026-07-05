// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use strand_dynamic_frame::DynamicFrameOwned;

mod movie_writer_thread;

/// Possible errors
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("mp4 writer error: {0}")]
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
    #[error("ffmpeg rewriter error {0}")]
    FfmpegReWriterError(#[from] ffmpeg_rewriter::Error),
    #[error("error loading CUDA or nvidia-encode: {0}")]
    NvEncLoad(String),
    #[error("error starting nvidia-encode: {0}")]
    NvEncStart(String),
}

type Result<T> = std::result::Result<T, Error>;

/// From outside the worker thread, check if we received an error from the
/// thread.
macro_rules! poll_err {
    ($err_rx: expr_2021) => {{
        // Recover from a poisoned lock rather than panicking: a panic here
        // would propagate out of `write`/`finish` and take down the process.
        if let Some(e) = $err_rx.lock().unwrap_or_else(|e| e.into_inner()).take() {
            return Err(e);
        }
    }};
}

/// A writer which will save a movie in a background thread.
///
/// [Self::new] will spawn the thread and the methods [Self::write] and
/// [Self::finish] return immediately, even though their work is not done.
pub struct BgMovieWriter {
    tx: std::sync::mpsc::SyncSender<Msg>,
    is_done: bool,
    err_from_worker: Arc<Mutex<Option<Error>>>,
}

impl BgMovieWriter {
    /// This spawns the writer thread.
    ///
    /// - `format_str_mp4` determines the filename used after formatting with
    ///   [chrono::DateTime::format].
    /// - `recording_config` specifies the recording method and configuration
    /// - `queue_size` is the number of frames that can be buffered before
    ///   frames will be dropped.
    /// - `data_dir`, if specified, will be the directory location of the saved
    ///   file.
    pub fn new(
        recording_config: strand_cam_remote_control::RecordingConfig,
        queue_size: usize,
        mp4_path: PathBuf,
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
            movie_writer_thread::writer_thread_loop(recording_config, err_to_launcher, rx, mp4_path)
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
    pub fn write<TS>(&mut self, frame: Arc<DynamicFrameOwned>, timestamp: TS) -> Result<()>
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
            // If the receiver has disconnected (e.g. the writer thread already
            // exited after an error), there is nothing to finish. Do not panic.
            if tx.send(Msg::Finish).is_err() {
                tracing::debug!("writer thread already gone; nothing to finish");
            }
        });
        Ok(())
    }
}

pub(crate) enum Msg {
    Write((Arc<DynamicFrameOwned>, chrono::DateTime<chrono::Local>)),
    Finish,
}

#[cfg(test)]
mod tests {
    use super::*;
    use machine_vision_formats::PixFmt;
    use strand_dynamic_frame::DynamicFrameOwned;

    /// Regression test: a writer error in the background thread must be
    /// reported as an `Err` from the launcher-side methods, never as a panic.
    ///
    /// Previously the error-reporting path panicked on the first error (and
    /// poisoned the shared error mutex), which propagated out and took down the
    /// whole process. Here we force `create_writer` to fail inside the worker
    /// thread by giving the ffmpeg writer a filename that does not end in
    /// `.mp4` (this fails before any external `ffmpeg` process is spawned).
    #[test]
    fn writer_error_is_reported_without_panic() {
        let cfg = strand_cam_remote_control::RecordingConfig::default();
        let bad_path = std::env::temp_dir().join("bg_movie_writer_test.not_mp4");
        let mut wtr = BgMovieWriter::new(cfg, 10, bad_path);

        let frame =
            Arc::new(DynamicFrameOwned::from_buf(4, 4, 4, vec![0u8; 16], PixFmt::Mono8).unwrap());
        let ts = chrono::Local::now();

        // Enqueue frames until the worker's error surfaces on the launcher
        // side. This must arrive as an `Err`, never as a panic.
        let mut got_err = false;
        for _ in 0..200 {
            if wtr.write(frame.clone(), ts).is_err() {
                got_err = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(got_err, "expected the writer error to be reported");

        // Further calls must still behave gracefully (the mutex must not be
        // poisoned) and must not panic.
        let _ = wtr.write(frame, ts);
        wtr.finish().unwrap();
    }
}
