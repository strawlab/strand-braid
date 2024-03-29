// Copyright 2022-2023 Andrew D. Straw.

use std::path::Path;

use color_eyre::{
    eyre::{self as anyhow, WrapErr},
    Result,
};

use crate::h264_source::H264Source;
use mp4::MediaType;

pub(crate) fn from_reader_with_timestamp_source<R: std::io::Read + std::io::Seek>(
    rdr: R,
    do_decode_h264: bool,
    size: u64,
    timestamp_source: crate::TimestampSource,
) -> Result<H264Source> {
    let mut mp4_reader = mp4::Mp4Reader::read_header(rdr, size)?;
    let timescale = mp4_reader.timescale();
    let mut video_track = None;
    for (track_id, track) in mp4_reader.tracks().iter() {
        // ignore all tracks except H264
        if track.media_type()? == MediaType::H264 {
            if video_track.is_some() {
                anyhow::bail!("only MP4 files with a single H264 video track are supported");
            }
            video_track = Some((track_id, track));
        }
    }

    let (track_id, track) = if let Some(vt) = video_track {
        vt
    } else {
        anyhow::bail!("No H264 video track found in MP4 file.");
    };

    let track_id = *track_id;

    let mut nal_units: Vec<Vec<u8>> = Vec::new();
    let mut mp4_pts = Vec::new();

    let mut sample_id = 1; // mp4 uses 1 based indexing
    let data_from_mp4_track = crate::h264_source::FromMp4Track {
        sequence_parameter_set: track.sequence_parameter_set()?.to_vec(),
        picture_parameter_set: track.picture_parameter_set()?.to_vec(),
    };
    while let Some(sample) = mp4_reader.read_sample(track_id, sample_id)? {
        if !sample.bytes.is_empty() {
            // dbg!((sample_id, mp4_pts, sample.bytes.len()));
            let sample_nal_units = avcc_to_nalu_ebsp(sample.bytes.as_ref())?;
            let n_nal_units = sample_nal_units.len();
            let this_pts = raw2dur(sample.start_time, timescale);
            for _ in 0..n_nal_units {
                mp4_pts.push(this_pts);
            }
            nal_units.extend(sample_nal_units.iter().map(|nal_unit| nal_unit.to_vec()));
        }
        sample_id += 1;
    }

    let h264_source = H264Source::from_nal_units_with_timestamp_source(
        nal_units,
        do_decode_h264,
        Some(mp4_pts),
        Some(data_from_mp4_track),
        timestamp_source,
    )?;
    Ok(h264_source)
}

pub fn from_path_with_timestamp_source<P: AsRef<Path>>(
    path: P,
    do_decode_h264: bool,
    timestamp_source: crate::TimestampSource,
) -> Result<H264Source> {
    let rdr = std::fs::File::open(path.as_ref())
        .with_context(|| format!("Opening {}", path.as_ref().display()))?;
    let size = rdr.metadata()?.len();
    let buf_reader = std::io::BufReader::new(rdr);

    let result =
        from_reader_with_timestamp_source(buf_reader, do_decode_h264, size, timestamp_source)
            .with_context(|| format!("Reading MP4 file {}", path.as_ref().display()))?;
    Ok(result)
}

/// Parse AVCC buffer to encapsulated bytes.
///
/// This is not capable of parsing on non-NALU boundaries and must contain
/// complete NALUs. For well-formed MP4 files, this should be the case.
fn avcc_to_nalu_ebsp(buf: &[u8]) -> Result<Vec<&[u8]>> {
    let mut result = vec![];
    let mut cur_buf = buf;
    while !cur_buf.is_empty() {
        if cur_buf.len() < 4 {
            anyhow::bail!("AVCC buffer too short");
        }
        let header = [cur_buf[0], cur_buf[1], cur_buf[2], cur_buf[3]];
        let sz: usize = u32::from_be_bytes(header).try_into().unwrap();
        let used = sz + 4;
        if cur_buf.len() < used {
            anyhow::bail!("AVCC buffer length: {sz}+4 but buffer {}", cur_buf.len());
        }
        result.push(&cur_buf[4..used]);
        cur_buf = &cur_buf[used..];
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
