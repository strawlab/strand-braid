//! This module contains [writer_thread_loop], the main loop for writing a movie
//! in a background thread. Everything here runs in one thread, and
//! [writer_thread_loop] should be called from a spawned thread.

use std::{
    fs::File,
    io::{Seek, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Local};
use mp4_writer::Mp4Writer;
use strand_cam_remote_control::FfmpegRecordingConfig;
use strand_dynamic_frame::DynamicFrame;

use crate::{Error, Msg, Result};

macro_rules! thread_try {
    ($xx: expr, $result: expr) => {{
        match $result {
            Ok(val) => val,
            Err(e) => {
                tracing::error!("{e}");
                // Clarify type
                let x: Arc<Mutex<Option<_>>> = $xx;
                // Send error. Panic if lock fails or previous error not sent.
                x.lock().unwrap().replace(e.into()).unwrap();
                return; // Exit the thread.
            }
        }
    }};
}

enum RawWriter<'lib, T>
where
    T: Write + Seek,
{
    Mp4Writer(Mp4Writer<'lib, T>),
    FfmpegReWriter(Box<MyFfmpegWriter>),
}

struct MyFfmpegWriter {
    inner: Option<ffmpeg_rewriter::FfmpegReWriter>,
}

impl MyFfmpegWriter {
    /// Save using ffmpeg to filename given.
    ///
    /// It is expected that the filename ends with '.mp4'.
    fn new<P: AsRef<Path>>(mp4_filename: P, cfg: &FfmpegRecordingConfig) -> Result<Self> {
        let mp4_filename: &Path = mp4_filename.as_ref();
        if mp4_filename.extension().and_then(|x| x.to_str()) != Some("mp4") {
            return Err(Error::FilenameDoesNotEndWithMp4);
        }
        let args = &cfg.codec_args;
        let ffmpeg_codec_args = ffmpeg_writer::FfmpegCodecArgs {
            device_args: args.device_args.clone(),
            codec: args.codec.clone(),
            pre_codec_args: args.pre_codec_args.clone(),
            post_codec_args: args.post_codec_args.clone(),
        };
        use strand_cam_remote_control::RecordingFrameRate::*;
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
        let fwtr = ffmpeg_rewriter::FfmpegReWriter::new(
            mp4_filename,
            ffmpeg_codec_args,
            rate,
            cfg.h264_metadata.clone(),
        )?;
        Ok(Self { inner: Some(fwtr) })
    }
    fn finish(&mut self) -> Result<()> {
        if let Some(fwtr) = self.inner.take() {
            fwtr.close()?;
        }
        Ok(())
    }
    fn write_dynamic(
        &mut self,
        frame: &DynamicFrame,
        timestamp: chrono::DateTime<chrono::Local>,
    ) -> Result<()> {
        if let Some(fwtr) = self.inner.as_mut() {
            fwtr.write_dynamic_frame(frame, timestamp)?;
        } else {
            return Err(Error::AlreadyClosed);
        };

        Ok(())
    }
}

/// Create a RawWriter. Runs inside writer thread loop.
fn create_writer<'a>(
    libs_result: &'a std::result::Result<nvenc::Dynlibs, nvenc::NvEncError>,
    recording_config: &strand_cam_remote_control::RecordingConfig,
    mp4_path: &'a Path,
) -> Result<RawWriter<'a, File>> {
    use strand_cam_remote_control::RecordingConfig::*;
    let raw: RawWriter<'_, File> = match &recording_config {
        Mp4(mp4_recording_config) => {
            let mp4_file = std::fs::File::create(&mp4_path)?;

            let nv_enc = match &mp4_recording_config.codec {
                strand_cam_remote_control::Mp4Codec::H264NvEnc(_opts) => {
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

            RawWriter::Mp4Writer(mp4_writer::Mp4Writer::new(
                mp4_file,
                mp4_recording_config.clone(),
                nv_enc,
            )?)
        }
        Ffmpeg(c) => RawWriter::FfmpegReWriter(Box::new(MyFfmpegWriter::new(&mp4_path, c)?)),
    };
    tracing::info!("Saving MP4 to \"{}\"", mp4_path.display());

    Ok(raw)
}

/// Save an image. Runs inside writer thread loop.
fn save_frame(
    raw: &mut RawWriter<'_, File>,
    frame: &DynamicFrame<'_>,
    stamp: DateTime<Local>,
    last_saved_stamp: &mut Option<DateTime<Local>>,
) -> Result<()> {
    match raw {
        RawWriter::Mp4Writer(ref mut r) => {
            r.write_dynamic(frame, stamp)?;
            *last_saved_stamp = Some(stamp);
        }
        RawWriter::FfmpegReWriter(ref mut r) => {
            r.write_dynamic(frame, stamp)?;
            *last_saved_stamp = Some(stamp);
        }
    }
    Ok(())
}

/// Finish the writer. Runs inside writer thread loop.
fn finish_writer(raw: &mut RawWriter<'_, File>) -> Result<()> {
    match raw {
        RawWriter::Mp4Writer(ref mut mp4_writer) => {
            mp4_writer.finish()?;
        }
        RawWriter::FfmpegReWriter(ffmpeg_wtr) => {
            ffmpeg_wtr.finish()?;
        }
    }
    Ok(())
}

pub(crate) fn writer_thread_loop(
    recording_config: strand_cam_remote_control::RecordingConfig,
    err_tx: Arc<Mutex<Option<Error>>>,
    rx: std::sync::mpsc::Receiver<Msg>,
    mp4_path: PathBuf,
) {
    {
        // Load CUDA and nvidia-encode shared libs, but do not return error
        // (yet).
        let libs_result = nvenc::Dynlibs::new();

        let mut raw: Option<RawWriter<'_, File>> = None;

        let mut last_saved_stamp: Option<chrono::DateTime<chrono::Local>> = None;

        loop {
            match rx.recv() {
                Ok(Msg::Write((frame, stamp))) => {
                    let raw_ref = if let Some(raw_ref) = raw.as_mut() {
                        raw_ref
                    } else {
                        let wtr = thread_try!(
                            err_tx,
                            create_writer(&libs_result, &recording_config, &mp4_path)
                        );
                        raw = Some(wtr);
                        raw.as_mut().unwrap()
                    };
                    let max_framerate = recording_config.max_framerate();
                    let do_save = match last_saved_stamp {
                        None => true,
                        Some(last_stamp) => {
                            let elapsed = stamp - last_stamp;
                            elapsed >= chrono::Duration::from_std(max_framerate.interval()).unwrap()
                        }
                    };
                    if do_save {
                        thread_try!(
                            err_tx,
                            save_frame(raw_ref, &frame.borrow(), stamp, &mut last_saved_stamp)
                        );
                    }
                }
                Ok(Msg::Finish) | Err(std::sync::mpsc::RecvError) => {
                    // Either an explicit Finish message was sent or the sender
                    // closed the channel. In either case, close the MP4 file.
                    if let Some(raw_ref) = raw.as_mut() {
                        thread_try!(err_tx, finish_writer(raw_ref));
                        tracing::info!("MP4 saving complete.");
                    } else {
                        tracing::error!("MP4 never started, but finish command received.");
                    }
                    return; // end the thread
                }
            };
        }
    }
}
