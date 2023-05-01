// Copyright 2022-2023 Andrew D. Straw.
use std::{
    io::{BufReader, Read, Seek},
    path::Path,
};

use anyhow::{Context, Result};

use h264_reader::{
    nal::{
        sei::{HeaderType, SeiMessage, SeiReader},
        slice::SliceHeader,
        Nal, RefNal, UnitType,
    },
    Context as H264ParsingContext,
};

use mp4::MediaType;

use ci2_remote_control::{H264Metadata, H264_METADATA_UUID, H264_METADATA_VERSION};

use super::*;

/// An MP4 file with H264 video data.
///
/// Strand Camera specific features are supported if present: metadata at the
/// start of the H264 stream is parsed and precision time stamps are read.
///
/// This should be as general purpose MP4 (h264) file converter as possible. MP4
/// files which do "strange" things (like having multiple video tracks or
/// setting H.264 SPS or PPS not only at the start of the video track) are not
/// supported.
pub struct Mp4H264Source<R: Read + Seek> {
    mp4_reader: mp4::Mp4Reader<R>,
    pub h264_metadata: Option<H264Metadata>,
    frame0_precision_time: Option<chrono::DateTime<chrono::Utc>>,
    track_id: u32,
    sample_count: u32,
    width: u32,
    height: u32,
    parsing_ctx: H264ParsingContext,
    _sps: h264_reader::nal::sps::SeqParameterSet,
    _pps: h264_reader::nal::pps::PicParameterSet,
    _mp4_rate: u32,
    h264_decoder_state: Option<openh264::decoder::Decoder>,
    last_decoded_sample: Option<u32>,
}

impl<R: Read + Seek> FrameDataSource for Mp4H264Source<R> {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn camera_name(&self) -> Option<&str> {
        self.h264_metadata
            .as_ref()
            .and_then(|x| x.camera_name.as_deref())
    }
    fn gamma(&self) -> Option<f32> {
        self.h264_metadata.as_ref().and_then(|x| x.gamma)
    }
    fn frame0_time(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        // In order of preference:
        //  - timestamp from Strand Camera's h264 metadata (with any timezone)
        //  - precision time stamp in UTC
        //  - None
        self.h264_metadata
            .as_ref()
            .map(|x| x.creation_time)
            .or_else(|| {
                self.frame0_precision_time.as_ref().map(|dt| {
                    let zero_offset = chrono::FixedOffset::east_opt(0).unwrap();
                    dt.with_timezone(&zero_offset)
                })
            })
    }
    fn skip_n_frames(&mut self, n_frames: usize) -> Result<()> {
        if n_frames > 0 {
            anyhow::bail!("Skipping frames with MP4 containing H264 codec is not supported.");
            // Doing so would require finding I frames and only skipping to
            // those (or decoding and interpolating a new I frame).
            // Also: caching SPS and PPS would be required.
            // We do this in the MKV reader, so we should use that
            // implementation for inspiration.
        }
        Ok(())
    }
    fn estimate_luminance_range(&mut self) -> Result<(u16, u16)> {
        anyhow::bail!("mp4 luminance scanning not implemented");
    }
    fn iter(&mut self) -> Box<dyn Iterator<Item = Result<FrameData>> + '_> {
        Box::new(Mp4Iter {
            parent: self,
            idx: 0,
        })
    }
}

struct Mp4Iter<'a, R: Read + Seek> {
    parent: &'a mut Mp4H264Source<R>,
    idx: usize,
}

impl<R: Read + Seek> Mp4H264Source<R> {
    fn new(rdr: R, size: u64, do_decode_h264: bool) -> Result<Self> {
        let mut mp4_reader = mp4::Mp4Reader::read_header(rdr, size)?;

        let _mp4_rate = mp4_reader.timescale();

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

        let mut sps = None;
        let mut pps = None;

        let (track_id, track) = if let Some((track_id, track)) = video_track {
            (track_id, track)
        } else {
            anyhow::bail!("No H264 video track found in MP4 file.");
        };

        // Note that these dimensions may be different from H264 dimensions
        // due to original material being, e.g. width 659 but H264 only supporting width 660,
        // in which case we should crop back to 659 here.
        let width_mp4 = u32::try_from(track.width()).unwrap();
        let height_mp4 = u32::try_from(track.height()).unwrap();

        let track_id = *track_id;

        let sample_count = mp4_reader.sample_count(track_id)?;
        if sample_count == 0 {
            anyhow::bail!("no samples in MP4 video track");
        }

        // read first frame (mp4 uses 1 based indexing)
        let sample = mp4_reader.read_sample(track_id, 1)?.unwrap();
        let nal_units = avcc_to_nalu_ebsp(sample.bytes.as_ref())?;

        let mut h264_metadata = None;
        let mut scratch = Vec::new();
        let mut parsing_ctx = H264ParsingContext::default();
        let mut frame0_precision_time = None;

        for nal_unit in nal_units {
            let nal = RefNal::new(nal_unit, &[], true);
            let nal_unit_type = nal.header().unwrap().nal_unit_type();
            match nal_unit_type {
                UnitType::SEI => {
                    let mut sei_reader = SeiReader::from_rbsp_bytes(nal.rbsp_bytes(), &mut scratch);
                    while let Some(sei_message) = sei_reader.next().unwrap() {
                        let udu = UserDataUnregistered::read(&sei_message)?;
                        match udu.uuid {
                            &H264_METADATA_UUID => {
                                let md: H264Metadata = serde_json::from_slice(udu.payload)?;
                                if md.version != H264_METADATA_VERSION {
                                    anyhow::bail!("unexpected version in h264 metadata");
                                }
                                if h264_metadata.is_some() {
                                    anyhow::bail!(
                                        "multiple SEI messages, but expected exactly one"
                                    );
                                }
                                h264_metadata = Some(md);
                            }
                            b"MISPmicrosectime" => {
                                frame0_precision_time = Some(parse_precision_time(udu.payload)?);
                            }
                            uuid => {
                                anyhow::bail!("unexpected SEI UDU UUID: {uuid:?}");
                            }
                        }
                    }
                }
                UnitType::SeqParameterSet => {
                    let isps =
                        h264_reader::nal::sps::SeqParameterSet::from_bits(nal.rbsp_bits()).unwrap();
                    if sps.is_some() {
                        anyhow::bail!("more than one SPS NAL unit not supported");
                    }
                    sps = Some(isps.clone());
                    // bit_depth_luma = Some(isps.chroma_info.bit_depth_luma_minus8 + 8);
                    parsing_ctx.put_seq_param_set(isps);
                }
                UnitType::PicParameterSet => {
                    let ipps = h264_reader::nal::pps::PicParameterSet::from_bits(
                        &parsing_ctx,
                        nal.rbsp_bits(),
                    )
                    .unwrap();
                    if pps.is_some() {
                        anyhow::bail!("more than one PPS NAL unit not supported");
                    }
                    pps = Some(ipps.clone());
                    parsing_ctx.put_pic_param_set(ipps);
                }
                _nal_unit_type => {}
            }
        }

        // We assume each MP4 sample contains exactly one image....

        let sps = sps.ok_or_else(|| anyhow::anyhow!("expected SPS not found"))?;
        let pps = pps.ok_or_else(|| anyhow::anyhow!("expected PPS not found"))?;

        let h264_decoder_state = if do_decode_h264 {
            Some(openh264::decoder::Decoder::new()?)
        } else {
            None
        };

        Ok(Self {
            mp4_reader,
            h264_metadata,
            frame0_precision_time,
            track_id,
            sample_count,
            width: width_mp4,
            height: height_mp4,
            parsing_ctx,
            _sps: sps,
            _pps: pps,
            _mp4_rate,
            h264_decoder_state,
            last_decoded_sample: None,
        })
    }

    /// Get a frame when we are sure sample_id is in the MP4 file.
    fn get_frame_inbounds(&mut self, sample_id: u32) -> Result<FrameData> {
        let sample = self
            .mp4_reader
            .read_sample(self.track_id, sample_id)?
            .ok_or_else(|| anyhow::anyhow!("no sample {sample_id} found"))?;

        let width = self.width;
        let height = self.height;

        // Set frame timestamp initially with MP4 data.
        let pts = raw2dur(sample.start_time);
        let host_timestamp = if let Some(f0_time) = self.frame0_time() {
            let pts_chrono = f0_time + chrono::Duration::from_std(pts)?;
            pts_chrono.with_timezone(&chrono::Utc)
        } else {
            todo!("how to get timestamp from video without one?");
        };
        let mut extra = Box::new(basic_frame::BasicExtra {
            host_timestamp,
            host_framenumber: sample_id as usize - 1,
        });

        let avcc_data: &[u8] = sample.bytes.as_ref();
        let nal_units = avcc_to_nalu_ebsp(avcc_data)?;
        let mut scratch = Vec::new();
        let mut precision_time = None;
        let mut image = None;
        for nal_unit in nal_units.iter() {
            let nal = RefNal::new(nal_unit, &[], true);
            let nal_unit_type = nal.header().unwrap().nal_unit_type();
            match nal_unit_type {
                UnitType::SeqParameterSet | UnitType::PicParameterSet => {
                    if sample_id != 1 {
                        // TODO (implementation started below):
                        // read it and only bail if the SPS or PPS changed.
                        anyhow::bail!(
                            "Unsupported: SPS or PPS during video (sample_id: {sample_id})"
                        );
                    }
                }
                // //  requires: h264-reader = {git="https://github.com/astraw/h264-reader", rev="7f896b2195d615976f2f57bd4a48c860c0d9ab35"}
                // UnitType::SeqParameterSet => {
                //     let sps =
                //         h264_reader::nal::sps::SeqParameterSet::from_bits(nal.rbsp_bits()).unwrap();

                //     if sps != self.sps {
                //         anyhow::bail!("Unsupported: changing SPS during video");
                //     }
                // }
                // UnitType::PicParameterSet => {
                //     let pps = h264_reader::nal::pps::PicParameterSet::from_bits(
                //         &self.parsing_ctx,
                //         nal.rbsp_bits(),
                //     )
                //     .unwrap();
                //     if pps != self.pps {
                //         anyhow::bail!("Unsupported: changing PPS during video");
                //     }
                // }
                UnitType::SliceLayerWithoutPartitioningIdr
                | UnitType::SliceLayerWithoutPartitioningNonIdr => {
                    match SliceHeader::from_bits(
                        &self.parsing_ctx,
                        &mut nal.rbsp_bits(),
                        nal.header().unwrap(),
                    ) {
                        Err(e) => {
                            anyhow::bail!("SliceHeaderError: {e:?}");
                        }
                        Ok((_sh, _sps, _pps)) => {
                            if let Some(decoder) = self.h264_decoder_state.as_mut() {
                                if nal_unit_type == UnitType::SliceLayerWithoutPartitioningNonIdr {
                                    // non keyframe, so must be in-order
                                    if let Some(last_decoded_sample) = self.last_decoded_sample {
                                        if last_decoded_sample + 1 != sample_id {
                                            anyhow::bail!("non-consecutive read on non-keyframe");
                                        }
                                    } else {
                                        anyhow::bail!("non-keyframe expected previous read");
                                    }
                                }

                                let annex_b = copy_nalus_to_annex_b(&nal_units);

                                let dynamic_frame = if let Some(decoded_yuv) =
                                    decoder.decode(&annex_b[..])?
                                {
                                    let dim = decoded_yuv.dimension_rgb();

                                    let stride = dim.0 * 3;
                                    let mut image_data = vec![0u8; stride * dim.1];
                                    decoded_yuv.write_rgb8(&mut image_data);
                                    basic_frame::DynamicFrame::RGB8(basic_frame::BasicFrame::<
                                        machine_vision_formats::pixel_format::RGB8,
                                    > {
                                        width,
                                        height,
                                        stride: u32::try_from(stride).unwrap(),
                                        image_data,
                                        pixel_format: std::marker::PhantomData,
                                        extra: extra.clone(),
                                    })
                                } else {
                                    anyhow::bail!("could not decode single frame with openh264");
                                };
                                image = Some(ImageData::Decoded(dynamic_frame));
                                self.last_decoded_sample = Some(sample_id);
                            } else {
                                image = Some(ImageData::EncodedH264(EncodedH264 {
                                    data: H264EncodingVariant::Avcc(avcc_data.to_vec()), // clone the data
                                    has_precision_timestamp: precision_time.is_some(), // precision time NAL comes before image NAL, so this is OK.
                                }));
                            }
                        }
                    }
                }
                UnitType::SEI => {
                    let mut sei_reader = SeiReader::from_rbsp_bytes(nal.rbsp_bytes(), &mut scratch);
                    while let Some(sei_message) = sei_reader.next().unwrap() {
                        let udu = UserDataUnregistered::read(&sei_message)?;
                        match udu.uuid {
                            b"MISPmicrosectime" => {
                                // Update frame timestamp with precision time.
                                let ts = parse_precision_time(udu.payload)?;
                                extra.host_timestamp = ts;
                                precision_time = Some(ts);
                            }
                            &H264_METADATA_UUID => {}
                            _uuid => {}
                        }
                    }
                }
                _nal_unit_type => {}
            }
        }

        let pts = match (&precision_time, self.frame0_precision_time) {
            (Some(frame_ts), Some(frame0_time)) => {
                let pts = *frame_ts - frame0_time;

                pts.to_std().map_err(|_| {
                    anyhow::anyhow!("could not convert chrono Duration to std Duration")
                })?
            }
            _ => {
                anyhow::bail!("not yet implemented: reading timestamps from MP4 data");
                // fn raw2dur(raw: u64, rate: u32) -> std::time::Duration {
                //     std::time::Duration::from_secs_f64(raw as f64 / rate as f64)
                // }
                // // This seems to give wrong timestamps. Perhaps a problem with
                // // the MP4 reader we are using?
                // raw2dur(sample.start_time, self.mp4_rate)
            }
        };
        let buf_len = avcc_data.len();
        let idx = usize::try_from(sample_id).unwrap() - 1;
        let image =
            image.ok_or_else(|| anyhow::anyhow!("no image found for sample_id {sample_id}"))?;
        Ok(FrameData {
            timestamp: pts,
            image,
            buf_len,
            idx,
        })
    }

    /// Get frame at index idx
    fn get_frame(&mut self, idx: usize) -> Option<Result<FrameData>> {
        // mp4 uses 1 based indexing
        let sample_id = u32::try_from(idx).unwrap() + 1;
        if sample_id > self.sample_count {
            None
        } else {
            Some(self.get_frame_inbounds(sample_id))
        }
    }
}

impl<'a, R: Read + Seek> Iterator for Mp4Iter<'a, R> {
    type Item = Result<FrameData>;
    fn next(&mut self) -> Option<Self::Item> {
        let result = self.parent.get_frame(self.idx);
        self.idx += 1;
        result
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::try_from(self.parent.sample_count).unwrap() - self.idx;
        (remaining, Some(remaining))
    }
}

pub fn from_path<P: AsRef<Path>>(
    path: P,
    do_decode_h264: bool,
) -> Result<Mp4H264Source<BufReader<std::fs::File>>> {
    let rdr = std::fs::File::open(path.as_ref())
        .with_context(|| format!("Opening {}", path.as_ref().display()))?;
    let size = rdr.metadata()?.len();
    let buf_reader = BufReader::new(rdr);
    Mp4H264Source::new(buf_reader, size, do_decode_h264)
        .with_context(|| format!("Reading MP4 file {}", path.as_ref().display()))
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
            anyhow::bail!("AVCC buffer length: {}+4 but buffer {}", sz, buf.len());
        }
        result.push(&cur_buf[4..used]);
        cur_buf = &cur_buf[used..];
    }
    Ok(result)
}

/// Copy AVCC to Annex B
fn copy_nalus_to_annex_b(nalus: &[&[u8]]) -> Vec<u8> {
    let sz = nalus.iter().fold(0, |acc, x| acc + x.len() + 4);
    let mut result = vec![0u8; sz];
    let mut start_idx = 0;
    for src in nalus.iter() {
        let dest = &mut result[start_idx..start_idx + 4 + src.len()];
        dest[3] = 0x01;
        dest[4..].copy_from_slice(src);
        start_idx += src.len() + 4;
    }
    result
}

struct UserDataUnregistered<'a> {
    pub uuid: &'a [u8; 16],
    pub payload: &'a [u8],
}

impl<'a> UserDataUnregistered<'a> {
    pub fn read(msg: &SeiMessage<'a>) -> Result<UserDataUnregistered<'a>> {
        if msg.payload_type != HeaderType::UserDataUnregistered {
            anyhow::bail!("expected UserDataUnregistered message");
        }
        if msg.payload.len() < 16 {
            anyhow::bail!("SEI payload too short to contain UserDataUnregistered message");
        }
        let uuid = (&msg.payload[0..16]).try_into().unwrap();

        let payload = &msg.payload[16..];
        Ok(UserDataUnregistered { uuid, payload })
    }
}

fn parse_precision_time(payload: &[u8]) -> Result<chrono::DateTime<chrono::Utc>> {
    if payload.len() != 12 {
        anyhow::bail!("unexpected payload length");
    }
    if payload[0] != 0x0F {
        anyhow::bail!("unexpected time stamp status byte");
    }
    let mut precision_time_stamp_bytes = [0u8; 8];
    for i in &[3, 6, 9] {
        if payload[*i] != 0xFF {
            anyhow::bail!("unexpected start code emulation prevention byte");
        }
    }
    precision_time_stamp_bytes[0..2].copy_from_slice(&payload[1..3]);
    precision_time_stamp_bytes[2..4].copy_from_slice(&payload[4..6]);
    precision_time_stamp_bytes[4..6].copy_from_slice(&payload[7..9]);
    precision_time_stamp_bytes[6..8].copy_from_slice(&payload[10..12]);
    let precision_time_stamp: i64 = i64::from_be_bytes(precision_time_stamp_bytes);
    let dur = chrono::Duration::microseconds(precision_time_stamp);

    let epoch_start = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
        .unwrap()
        .and_hms_micro_opt(0, 0, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Utc)
        .unwrap();

    Ok(epoch_start + dur)
}

// The number of time units that pass in one second.
const MOVIE_TIMESCALE: u32 = 1_000_000;

fn raw2dur(raw: u64) -> std::time::Duration {
    std::time::Duration::from_secs_f64(raw as f64 / MOVIE_TIMESCALE as f64)
}

#[test]
fn test_raw_duration() {
    fn dur2raw(dur: &std::time::Duration) -> u64 {
        (dur.as_secs_f64() * MOVIE_TIMESCALE as f64).round() as u64
    }

    fn roundtrip(orig: u64) {
        let actual = dur2raw(&raw2dur(orig));
        assert_eq!(orig, actual);
    }
    roundtrip(0);
    roundtrip(100);
    roundtrip(1_000_000);
    roundtrip(1_000_000_000);
    roundtrip(1_000_000_000_000);
}
