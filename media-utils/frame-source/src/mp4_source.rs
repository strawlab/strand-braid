// Copyright 2022-2024 Andrew D. Straw.

use std::path::Path;

use crate::{
    h264_source::{H264Source, SeekRead, SeekableH264Source},
    Result,
};
use mp4::MediaType;

#[derive(Debug, Clone, PartialEq)]
pub struct Mp4NalLocation {
    track_id: u32,
    sample_id: u32,
}

#[derive(thiserror::Error, Debug)]
pub enum Mp4SourceError {
    #[error("sample is empty")]
    SampleEmpty,
    #[error("sample in track disappeared")]
    SampleDisappeared,
    #[error("only MP4 files with a single H264 video track are supported")]
    SingleH264TrackOnly,
    #[error("No H264 video track found in MP4 file.")]
    NoH264Track,
    #[error("sample buffer is too short for NAL unit header")]
    SampleBufferTooShort,
    #[error("AVCC buffer length: {sz}+4 but buffer {cur_len}")]
    LengthMismatch { sz: usize, cur_len: usize },
}

pub struct Mp4Source {
    mp4_reader: mp4::Mp4Reader<Box<dyn SeekRead + Send>>,
    nal_locations: Vec<Mp4NalLocation>,
    first_sps: Vec<u8>,
    first_pps: Vec<u8>,
}

impl SeekableH264Source for Mp4Source {
    type NalLocation = Mp4NalLocation;
    fn nal_boundaries(&mut self) -> &[Self::NalLocation] {
        &self.nal_locations
    }
    fn read_nal_units_at_location(&mut self, location: &Self::NalLocation) -> Result<Vec<Vec<u8>>> {
        if let Some(sample) = self
            .mp4_reader
            .read_sample(location.track_id, location.sample_id)?
        {
            if !sample.bytes.is_empty() {
                let sample_nal_units = avcc_to_nalu_ebsp(sample.bytes.as_ref())?;
                Ok(sample_nal_units.iter().map(|x| x.to_vec()).collect())
            } else {
                Err(Mp4SourceError::SampleEmpty.into())
            }
        } else {
            Err(Mp4SourceError::SampleDisappeared.into())
        }
    }
    fn first_sps(&self) -> Option<Vec<u8>> {
        Some(self.first_sps.clone())
    }
    fn first_pps(&self) -> Option<Vec<u8>> {
        Some(self.first_pps.clone())
    }
}

pub(crate) fn from_reader_with_timestamp_source(
    mut mp4_reader: mp4::Mp4Reader<Box<dyn SeekRead + Send>>,
    do_decode_h264: bool,
    timestamp_source: crate::TimestampSource,
    srt_file_path: Option<std::path::PathBuf>,
) -> Result<H264Source<Mp4Source>> {
    let timescale = mp4_reader.timescale();
    let mut video_track = None;
    for (track_id, track) in mp4_reader.tracks().iter() {
        // ignore all tracks except H264
        if track.media_type()? == MediaType::H264 {
            if video_track.is_some() {
                return Err(Mp4SourceError::SingleH264TrackOnly.into());
            }
            video_track = Some((track_id, track));
        }
    }

    let (track_id, track) = if let Some(vt) = video_track {
        vt
    } else {
        return Err(Mp4SourceError::NoH264Track.into());
    };

    let track_id = *track_id;

    // Iterate over every sample in the track. Typically (always?) one such MP4
    // sample corresponds to one frame of video (and often multiple NAL units).
    // Here we assume this 1:1 mapping between MP4 samples and video frames. The
    // `nal_locations` and `mp4_pts` each are indexed by sample number.
    let mut nal_locations = Vec::new();
    let mut mp4_pts = Vec::new();
    let data_from_mp4_track = crate::h264_source::FromMp4Track {
        sequence_parameter_set: track.sequence_parameter_set()?.to_vec(),
        picture_parameter_set: track.picture_parameter_set()?.to_vec(),
    };
    let num_samples = mp4_reader.sample_count(track_id)?;

    // mp4 uses 1 based indexing
    for sample_id in 1..=num_samples {
        let (start_time, _duration) = mp4_reader.sample_time_duration(track_id, sample_id)?;
        let this_pts = raw2dur(start_time, timescale);
        mp4_pts.push(this_pts);
        nal_locations.push(Mp4NalLocation {
            track_id,
            sample_id,
        });
    }
    assert_eq!(mp4_pts.len(), num_samples as usize);

    let seekable_h264_source = Mp4Source {
        mp4_reader,
        nal_locations,
        first_sps: data_from_mp4_track.sequence_parameter_set.clone(),
        first_pps: data_from_mp4_track.picture_parameter_set.clone(),
    };

    let h264_source = H264Source::from_seekable_h264_source_with_timestamp_source(
        seekable_h264_source,
        do_decode_h264,
        Some(mp4_pts),
        Some(data_from_mp4_track),
        timestamp_source,
        srt_file_path,
    )?;
    Ok(h264_source)
}

pub fn from_path_with_timestamp_source<P: AsRef<Path>>(
    path: P,
    do_decode_h264: bool,
    timestamp_source: crate::TimestampSource,
    srt_file_path: Option<std::path::PathBuf>,
) -> Result<H264Source<Mp4Source>> {
    let rdr = std::fs::File::open(path.as_ref())?;
    let size = rdr.metadata()?.len();
    let buf_reader: Box<(dyn SeekRead + Send + 'static)> = Box::new(std::io::BufReader::new(rdr));
    let mp4_reader = mp4::Mp4Reader::read_header(buf_reader, size)?;

    let result = from_reader_with_timestamp_source(
        mp4_reader,
        do_decode_h264,
        timestamp_source,
        srt_file_path,
    )?;
    Ok(result)
}

/// Parse sample from MP4 as NAL units.
///
/// In MP4 files, each sample buffer is multiple NAL units consisting of a
/// 4-byte length header and the data.
///
/// This function is not capable of parsing on non-NALU boundaries and must
/// contain complete NALUs. For well-formed MP4 files, this should be the case.
fn avcc_to_nalu_ebsp(mp4_sample_buffer: &[u8]) -> Result<Vec<&[u8]>> {
    let mut result = vec![];
    let mut cur_buf = mp4_sample_buffer;
    let mut total_nal_sizes = 0;
    while !cur_buf.is_empty() {
        if cur_buf.len() < 4 {
            return Err(Mp4SourceError::SampleBufferTooShort.into());
        }
        let header = [cur_buf[0], cur_buf[1], cur_buf[2], cur_buf[3]];
        let sz: usize = u32::from_be_bytes(header).try_into().unwrap();
        let used = sz + 4;
        if cur_buf.len() < used {
            return Err(Mp4SourceError::LengthMismatch {
                sz,
                cur_len: cur_buf.len(),
            }
            .into());
        }
        total_nal_sizes += used;
        result.push(&cur_buf[4..used]);
        cur_buf = &cur_buf[used..];
    }
    if total_nal_sizes != mp4_sample_buffer.len() {
        tracing::warn!(
            "MP4 sample was {} bytes, but H264 NAL units totaled {} bytes.",
            mp4_sample_buffer.len(),
            total_nal_sizes
        );
    }
    Ok(result)
}

fn raw2dur(raw: u64, timescale: u32) -> std::time::Duration {
    std::time::Duration::from_secs_f64(raw as f64 / timescale as f64)
}

#[test]
fn test_raw_duration() {
    const TIMESCALE: u32 = 90_000;
    fn dur2raw(dur: &std::time::Duration) -> u64 {
        (dur.as_secs_f64() * TIMESCALE as f64).round() as u64
    }

    fn roundtrip(orig: u64) {
        let actual = dur2raw(&raw2dur(orig, TIMESCALE));
        assert_eq!(orig, actual);
    }
    roundtrip(0);
    roundtrip(100);
    roundtrip(1_000_000);
    roundtrip(1_000_000_000);
    roundtrip(1_000_000_000_000);
}
