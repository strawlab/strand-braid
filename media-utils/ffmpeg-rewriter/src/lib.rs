// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use chrono::{DateTime, Local};
use frame_source::{FrameDataSource, h264_source::SeekableH264Source};
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

        // Choose filename that makes conflict unlikely if the user also writes
        // an SRT file. They are likely to use "{basename}.srt" as this will
        // then play in VLC and likely other players. As this SRT file is only
        // temporary, it doesn't matter much what exactly it is called, but it
        // shouldn't have a high likelihood of conflict.
        let srt_file_path = format!("{basename}-ffmpeg-rewriter.srt");
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
            "Copying original .mp4 file into new .mp4 files with timestamps and metadata. frame0_time: {frame0_time}, mp4_cfg: {:?}",
            self.mp4_cfg
        );
        let fd = std::fs::File::create(&fname2)?;
        let mut new_mp4 = mp4_writer::Mp4Writer::new(fd, self.mp4_cfg, None)?;
        let h264_src = frame_src.as_seekable_h264_source();
        new_mp4.set_first_sps_pps(h264_src.first_sps(), h264_src.first_pps());

        let insert_precision_timestamp = true;
        let width = frame_src.width();
        let height = frame_src.height();

        // Snapshot the source's per-sample timing (stts + ctts) before
        // iterating (which borrows `frame_src` mutably). Preserving this timing
        // verbatim is what keeps reordered (B-frame) streams correct: the
        // container ordering comes from the source, while the precise capture
        // time is carried per-frame in the precision-timestamp SEI.
        let sample_timing: Option<Vec<_>> = frame_src.mp4_sample_timing().map(|t| t.to_vec());

        let mut count = 0;
        for frame in frame_src.decode_order_iter() {
            let frame = frame?;
            let timestamp = frame0_time + frame.timestamp().unwrap_duration();
            let idx = frame.idx();
            let data = match frame.image() {
                frame_source::ImageData::EncodedH264(data) => &data.data,
                _ => {
                    return Err(Error::SourceIsNotH264);
                }
            };
            match sample_timing.as_ref().and_then(|t| t.get(idx)) {
                Some(st) => new_mp4.write_h264_buf_passthrough(
                    data,
                    width,
                    height,
                    st.decode_duration,
                    st.composition_offset,
                    timestamp,
                    insert_precision_timestamp,
                )?,
                None => new_mp4.write_h264_buf(
                    data,
                    width,
                    height,
                    timestamp,
                    frame0_time,
                    insert_precision_timestamp,
                )?,
            }
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

        // Frames are read back in decode order, which differs from
        // presentation (input) order for B-frame streams. Every precise
        // timestamp must nonetheless round-trip intact; the per-frame SEI
        // carries each frame's own capture time regardless of decode order.
        // (Correct playback *ordering* is covered by the end-to-end smoke test.)
        let mut got: Vec<_> = Vec::new();
        for frame in frame_src.decode_order_iter() {
            let frame = frame?;
            got.push(frame0_time + frame.timestamp().unwrap_duration());
        }
        assert_eq!(got.len(), timestamps.len());
        got.sort();
        let mut expected = timestamps.clone();
        expected.sort();
        assert_eq!(got, expected);

        Ok(())
    }

    /// Regression test for re-muxing a reordered (B-frame) stream so that it
    /// plays back in the correct presentation order.
    ///
    /// The intermediate libx264 pass stores frames in *decode* order with
    /// non-zero composition offsets (B-frames). Before the fix, [`FfmpegReWriter`]
    /// could not represent this: `mp4-writer` hardcoded a zero composition
    /// offset (no `ctts`) and paired each SRT capture time with the frame by
    /// decode index, so the re-muxed file had its capture times scrambled
    /// relative to the true display order (frames played e.g. 5,1,2,3,4,...).
    ///
    /// Here we force B-frames, re-mux, and then read the result back. We
    /// reconstruct each sample's presentation time from the container timing
    /// (`stts` decode duration + `ctts` composition offset) and assert that,
    /// walked in presentation order, the per-frame precision-timestamp SEI
    /// capture times are strictly increasing — i.e. the file plays in order.
    /// Prior to the fix (composition offset forced to zero, SEI paired by
    /// decode index) this ordering was violated.
    #[test]
    fn test_bframe_stream_remuxes_in_presentation_order() -> Result<()> {
        use frame_source::h264_source::Mp4SampleTiming;

        let tempdir = tempfile::tempdir()?;
        let mp4_fname = tempdir.path().join("out.mp4");

        // 25 fps nominal cadence; the SRT carries the real (here identical)
        // capture times.
        let n_frames = 24usize;
        let base_micros: i64 = 1_662_921_288_000_000; // Sun, 11 Sep 2022 18:34:48 UTC
        let frame_interval_micros = 40_000i64; // 25 fps
        let timestamps: Vec<_> = (0..n_frames)
            .map(|i| {
                DateTime::from_timestamp_micros(base_micros + i as i64 * frame_interval_micros)
                    .unwrap()
            })
            .collect();

        let w = 64u32;
        let h = 48u32;
        {
            // Force libx264 to insert a fixed pattern of B-frames (b_adapt=0
            // takes the content out of the decision) so the re-mux definitely
            // exercises the reordered path. A single keyframe keeps one GOP.
            let ffmpeg_codec_args = FfmpegCodecArgs {
                device_args: None,
                pre_codec_args: None,
                codec: Some("libx264".to_string()),
                post_codec_args: Some(vec![
                    ("-bf".to_string(), "3".to_string()),
                    (
                        "-x264-params".to_string(),
                        "b_adapt=0:scenecut=0:keyint=1000:min-keyint=1000".to_string(),
                    ),
                ]),
                pixfmt: Some("yuv420p".to_string()),
                // This test deliberately forces B-frames via `-bf 3` in
                // `post_codec_args` to exercise reordering, so do not emit the
                // default `-bf 0`.
                max_bframes: None,
            };

            let mut wtr = FfmpegReWriter::new(&mp4_fname, ffmpeg_codec_args, None, None)?;

            for (i, ts) in timestamps.iter().enumerate() {
                // Vary the content per frame so the encoder has real motion to
                // reorder around.
                let mut data = vec![0u8; w as usize * h as usize * 3];
                for (px, chunk) in data.chunks_exact_mut(3).enumerate() {
                    let v = ((px + i * 7) % 256) as u8;
                    chunk[0] = v;
                    chunk[1] = v.wrapping_mul(3);
                    chunk[2] = v.wrapping_add(i as u8 * 11);
                }
                let frame: OImage<RGB8> = OImage::new(w, h, w as usize * 3, data).unwrap();
                let frame = strand_dynamic_frame::DynamicFrameOwned::from_static(frame);
                wtr.write_dynamic_frame(&frame.borrow(), *ts)?;
            }
            wtr.close()?;
        }

        // Read the re-muxed file back, keeping the H264 in decode order and
        // recovering the container timing so we can reconstruct presentation
        // order.
        let mut frame_src = frame_source::FrameSourceBuilder::new(&mp4_fname)
            .do_decode_h264(false)
            .timestamp_source(frame_source::TimestampSource::MispMicrosectime)
            .build_h264_in_mp4_source()?;

        let frame0_time = frame_src.frame0_time().unwrap();

        // Snapshot per-sample timing (stts + ctts) before iterating (which
        // borrows the source mutably).
        let sample_timing: Vec<Mp4SampleTiming> = frame_src
            .mp4_sample_timing()
            .expect("MP4 source must expose per-sample timing")
            .to_vec();
        assert_eq!(sample_timing.len(), n_frames);

        // The re-mux is only meaningful as a reordering test if the encoder
        // actually produced B-frames (non-zero composition offsets).
        let has_reordering = sample_timing
            .iter()
            .any(|t| t.composition_offset != chrono::Duration::zero());
        assert!(
            has_reordering,
            "expected libx264 to emit B-frames (non-zero ctts); test would be vacuous otherwise"
        );

        // Collect the SEI capture time for each sample, in decode order.
        let mut sei_times = vec![None; n_frames];
        for frame in frame_src.decode_order_iter() {
            let frame = frame?;
            sei_times[frame.idx()] = Some(frame0_time + frame.timestamp().unwrap_duration());
        }

        // Reconstruct each sample's presentation time: presentation = decode +
        // composition_offset, where the decode time is the running sum of the
        // per-sample decode durations (stts), all in decode order.
        let mut decode_time = chrono::Duration::zero();
        let mut presentation = Vec::with_capacity(n_frames);
        for (i, timing) in sample_timing.iter().enumerate() {
            let pts = decode_time
                + chrono::Duration::from_std(timing.decode_duration).unwrap()
                + timing.composition_offset;
            let sei = sei_times[i].expect("every sample must carry a SEI timestamp");
            presentation.push((pts, sei));
            decode_time += chrono::Duration::from_std(timing.decode_duration).unwrap();
        }

        // Walk samples in presentation order and assert the SEI capture times
        // are strictly increasing: the file plays back in the order it was
        // recorded.
        presentation.sort_by_key(|(pts, _)| *pts);
        let ordered_sei: Vec<_> = presentation.iter().map(|(_, sei)| *sei).collect();
        for pair in ordered_sei.windows(2) {
            assert!(
                pair[0] < pair[1],
                "SEI capture times must strictly increase in presentation order, \
                 but got {:?} then {:?} (out-of-order playback)",
                pair[0],
                pair[1]
            );
        }

        // And the set of capture times must match what we wrote.
        assert_eq!(ordered_sei, timestamps);

        Ok(())
    }
}
