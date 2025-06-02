use chrono::{DateTime, Local};
use frame_source::{h264_source::SeekableH264Source, FrameDataSource};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use ffmpeg_writer::{FfmpegCodecArgs, FfmpegWriter};
use strand_cam_remote_control::{H264Metadata, Mp4Codec, Mp4RecordingConfig, RecordingFrameRate};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ffmpeg_writer {0}")]
    FfmpegWriter(#[from] ffmpeg_writer::Error),
    #[error("cannot reencode")]
    CannotReencode,
    #[error("filename does not end with '.mp4'")]
    FilenameDoesNotEndWithMp4,
    #[error("filename not unicode")]
    FilenameNotUnicode,
    #[error("source does not contain H264 video")]
    SourceIsNotH264,
    #[error("MP4 writer error: {0}")]
    Mp4WriterError(#[from] mp4_writer::Error),
    #[error("frame source error: {0}")]
    FrameSourceError(#[from] frame_source::Error),
    #[error("serde json error: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Deserialize)]
struct SrtMsg {
    timestamp: DateTime<chrono::Local>,
}

/// Save to a video using [FfmpegWriter] but when done, read the newly-written
/// file and resave the data (without transcoding) with timestamps and other
/// metadata.
pub struct FfmpegReWriter {
    mp4_filename: String,
    ffmpeg_wtr: FfmpegWriter,
    mp4_cfg: Mp4RecordingConfig,
    srt_file_path: String,
    swtr: srt_writer::BufferingSrtFrameWriter,
    json_file_path: Option<String>,
}

impl FfmpegReWriter {
    pub fn new(
        mp4_path: impl AsRef<std::path::Path>,
        ffmpeg_codec_args: FfmpegCodecArgs,
        rate: Option<(usize, usize)>,
        h264_metadata: Option<H264Metadata>,
    ) -> Result<Self> {
        tracing::debug!(
            "Creating FfmpegReWriter for {} with h264_metadata: {h264_metadata:?}",
            mp4_path.as_ref().display()
        );
        let mp4_filename = PathBuf::from(mp4_path.as_ref())
            .into_os_string()
            .into_string()
            .map_err(|_| Error::FilenameNotUnicode)?;
        let basename = if let Some(basename) = mp4_filename.strip_suffix(".mp4") {
            basename
        } else {
            return Err(Error::FilenameDoesNotEndWithMp4);
        };

        let srt_file_path = format!("{basename}.srt");
        let json_file_path = if let Some(h264_metadata) = &h264_metadata {
            // Save the metadata to a file in case we crash before
            // Self::close(). That way this information can be recovered.
            let jpath = format!("{basename}-metadata.json");
            let buf = serde_json::to_string(h264_metadata)?;
            std::fs::write(&jpath, buf)?;
            Some(jpath)
        } else {
            None
        };

        let ffmpeg_wtr = FfmpegWriter::new(&mp4_filename, ffmpeg_codec_args, rate)?;
        let mp4_cfg = Mp4RecordingConfig {
            codec: Mp4Codec::H264RawStream,
            max_framerate: RecordingFrameRate::Unlimited,
            h264_metadata: h264_metadata.clone(),
        };

        let out_fd = std::fs::File::create(&srt_file_path)?;
        let swtr = srt_writer::BufferingSrtFrameWriter::new(Box::new(out_fd));
        Ok(Self {
            mp4_filename: mp4_filename.to_string(),
            ffmpeg_wtr,
            mp4_cfg,
            srt_file_path,
            swtr,
            json_file_path,
        })
    }

    /// Write a frame and timestamp.
    pub fn write_dynamic_frame<TS>(
        &mut self,
        frame: &strand_dynamic_frame::DynamicFrame,
        timestamp: TS,
    ) -> Result<()>
    where
        TS: Into<DateTime<Local>>,
    {
        let timestamp = timestamp.into();

        let mp4_pts = self
            .ffmpeg_wtr
            .write_dynamic_frame(frame)
            .map_err(Error::FfmpegWriter)?;

        let msg = SrtMsg { timestamp };
        let msg = serde_json::to_string(&msg).unwrap();
        self.swtr.add_frame(mp4_pts, msg)?;
        self.swtr.flush()?;

        Ok(())
    }

    pub fn close(self) -> Result<()> {
        // finish with ffmpeg and finish writing SRT
        self.ffmpeg_wtr.close()?;
        self.swtr.close()?;
        tracing::debug!("Done creating original .mp4 and .srt files.");

        // Create reader for h264 data from .mp4 and timestamps from .srt.
        let mut frame_src = frame_source::FrameSourceBuilder::new(&self.mp4_filename)
            .do_decode_h264(false)
            .timestamp_source(frame_source::TimestampSource::SrtFile)
            .srt_file_path(Some(PathBuf::from(&self.srt_file_path)))
            .build_h264_in_mp4_source()?;

        let frame0_time = frame_src.frame0_time().unwrap();

        // Create new .mp4 file, also with original h264 metadata.
        let fname2 = format!("{}-rewritten.mp4", self.mp4_filename);
        tracing::debug!(
            "Copying original .mp4 file into new .mp4 files with timestamps and metadata. frame0_time: {frame0_time}, mp4_cfg: {:?}", self.mp4_cfg
        );
        let fd = std::fs::File::create(&fname2)?;
        let mut new_mp4 = mp4_writer::Mp4Writer::new(fd, self.mp4_cfg, None)?;
        let h264_src = frame_src.as_seekable_h264_source();
        new_mp4.set_first_sps_pps(h264_src.first_sps(), h264_src.first_pps());

        let insert_precision_timestamp = true;
        let width = frame_src.width();
        let height = frame_src.height();

        let mut count = 0;
        for frame in frame_src.iter() {
            let frame = frame?;
            let timestamp = frame0_time + frame.timestamp().unwrap_duration();
            let data = match frame.image() {
                frame_source::ImageData::EncodedH264(data) => &data.data,
                _ => {
                    return Err(Error::SourceIsNotH264);
                }
            };
            new_mp4.write_h264_buf(
                data,
                width,
                height,
                timestamp,
                frame0_time,
                insert_precision_timestamp,
            )?;
            count += 1;
        }

        new_mp4.finish()?;
        tracing::debug!("Finished writing new .mp4 file with {count} frames.");

        tracing::debug!(
            "Renaming new .mp4 file to original .mp4 \
            name, thereby deleting original."
        );
        std::fs::rename(fname2, self.mp4_filename)?;

        // Remove no longer need .srt and .json files.
        std::fs::remove_file(&self.srt_file_path)?;
        if let Some(jpath) = self.json_file_path {
            std::fs::remove_file(jpath)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use machine_vision_formats::{owned::OImage, pixel_format::RGB8};

    use test_log::test;

    #[test]
    fn test_ffmpeg_rewriter() -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mp4_fname = tempdir.path().join("out.mp4");

        // let mp4_fname = "out.mp4";

        let timestamp_micros: i64 = 1_662_921_288_000_000; // Sun, 11 Sep 2022 18:34:48 UTC

        let mut timestamps = vec![
            DateTime::from_timestamp_micros(timestamp_micros).unwrap(),
            DateTime::from_timestamp_micros(timestamp_micros + 1).unwrap(),
            DateTime::from_timestamp_micros(timestamp_micros + 100).unwrap(),
        ];

        for delta in 1..10 {
            let micros = delta * 10_000;
            timestamps.push(DateTime::from_timestamp_micros(timestamp_micros + micros).unwrap());
        }

        tracing::debug!("Encoding {} frames", timestamps.len());

        let w = 640;
        let h = 480;
        {
            // let ffmpeg_codec_args = ffmpeg_writer::platform_hardware_encoder()?;
            let ffmpeg_codec_args = Default::default();

            let rate = None;
            let h264_metadata = None;
            let mut wtr = FfmpegReWriter::new(&mp4_fname, ffmpeg_codec_args, rate, h264_metadata)?;

            for (i, ts) in timestamps.iter().enumerate() {
                let value = (i % 255) as u8;
                let frame: OImage<RGB8> = OImage::new(
                    w,
                    h,
                    w as usize * 3,
                    vec![value; w as usize * h as usize * 3],
                )
                .unwrap();
                let frame = strand_dynamic_frame::DynamicFrameOwned::from_static(frame);
                wtr.write_dynamic_frame(&frame.borrow(), *ts)?;
            }
            wtr.close()?;
        }

        let mut frame_src = frame_source::FrameSourceBuilder::new(&mp4_fname)
            .do_decode_h264(false)
            .timestamp_source(frame_source::TimestampSource::MispMicrosectime)
            .build_source()?;

        let frame0_time = frame_src.frame0_time().unwrap();
        assert_eq!(frame0_time, timestamps[0]);

        assert_eq!(frame_src.width(), w);
        assert_eq!(frame_src.height(), h);

        let mut count = 0;
        for (i, frame) in frame_src.iter().enumerate() {
            let frame = frame?;
            let timestamp = frame0_time + frame.timestamp().unwrap_duration();
            assert_eq!(timestamps[i], timestamp);
            count += 1;
        }
        assert_eq!(count, timestamps.len());

        Ok(())
    }
}
