// Copyright 2022-2023 Andrew D. Straw.
use std::{
    io::{BufReader, Read, Seek},
    path::Path,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use h264_reader::{
    nal::{
        sei::{HeaderType, SeiMessage, SeiReader},
        Nal, RefNal, UnitType,
    },
    rbsp::BitReaderError,
    Context as H264ParsingContext,
};

use ci2_remote_control::{H264Metadata, H264_METADATA_UUID, H264_METADATA_VERSION};

use super::*;

/// H264 data source. Can come directly from an "Annex B" format .h264 file or
/// from an MP4 file.
///
/// This should be as general purpose H264 file reader as possible.
///
/// Strand Camera specific features are supported if present: metadata at the
/// H264 stream start (UUID 0ba99cc7-f607-3851-b35e-8c7d8c04da0a) is parsed, as
/// are precision time stamps (specified by MISB ST 0604.3).
///
/// ## Timestamp handling:
///
/// ### Case 1, H264 in MP4
///
/// Tracks in MP4 files are composed of samples. Each sample has a presentation
/// timestamp (PTS), the time elapsed from the start. A sample contains multiple
/// (0, 1, 2 or more) H264 NAL units. As far as I understand, each sample should
/// contain exactly one image frame of data. (That said, there exist MP4 files
/// in which a sample contains zero NAL units and thus zero image frames.)
/// Samples also carry a duration, which is informational to assist with
/// playback. The duration of frame N should be the PTS of frame N+1 - the PTS
/// of frame N. There seems to be a general assumption that samples should be
/// equi-distant in time and thus that the file has a constant frame rate,
/// although I have not found this in any specification.
///
/// ### Case 2, raw H264
///
/// A raw .h264 file, which is defined as the "Annex B format", (or simply the
/// H264 data inside an MP4 file) may have no explicit timing information in it.
/// Alternatively, for example with the `timing_info_present_flag` in the VUI
/// parameters, timing information may be present. So far we ignore these flags.
/// In this case, the timestamp data for the frame is simply returned as a
/// fraction of complete (in the interval from 0.0 to 1.0).
///
/// ### Case 3, H264 files with `MISPmicrosectime` supplemental enhancement information
///
/// H264 can contain additional NAL units, called supplemental enhancement
/// information (SEI) which is ignored by decoders but can provide additional
/// information such as metadata at the start of H264 data and per-frame
/// timestamps as specified in MISB ST 0604.3.
pub struct H264Source {
    nal_units: Vec<Vec<u8>>,
    /// timestamps from MP4 files, per NAL unit
    mp4_pts: Option<Vec<std::time::Duration>>,
    frame_to_nalu_index: Vec<(usize, Option<DateTime<Utc>>)>,
    pub h264_metadata: Option<H264Metadata>,
    frame0_precision_time: Option<chrono::DateTime<chrono::FixedOffset>>,
    width: u32,
    height: u32,
    do_decode_h264: bool,
    timestamp_source: &'static str,
    has_timestamps: bool,
}

impl FrameDataSource for H264Source {
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
        self.frame0_precision_time
    }
    fn skip_n_frames(&mut self, n_frames: usize) -> Result<()> {
        if n_frames > 0 {
            anyhow::bail!("Skipping frames with H264 file is not supported.");
            // Doing so would require finding I frames and only skipping to
            // those (or decoding and interpolating a new I frame).
            // Also: caching SPS and PPS would be required.
            // We do this in the MKV reader, so we should use that
            // implementation for inspiration.
        }
        Ok(())
    }
    fn estimate_luminance_range(&mut self) -> Result<(u16, u16)> {
        anyhow::bail!("h264 luminance scanning not implemented");
    }
    fn iter(&mut self) -> Box<dyn Iterator<Item = Result<FrameData>> + '_> {
        let openh264_decoder_state = if self.do_decode_h264 {
            Some(openh264::decoder::Decoder::new().unwrap())
        } else {
            None
        };
        Box::new(RawH264Iter {
            parent: self,
            frame_idx: 0,
            next_nal_idx: 0,
            openh264_decoder_state,
        })
    }
    fn timestamp_source(&self) -> &str {
        self.timestamp_source
    }
    fn has_timestamps(&self) -> bool {
        self.has_timestamps
    }
}

pub(crate) struct FromMp4Track {
    pub(crate) sequence_parameter_set: Vec<u8>,
    pub(crate) picture_parameter_set: Vec<u8>,
}

impl H264Source {
    /// `nal_units` are EBSP without Annex B or AVCC headers.
    pub(crate) fn from_nal_units(
        nal_units: Vec<Vec<u8>>,
        do_decode_h264: bool,
        mp4_pts: Option<Vec<std::time::Duration>>,
        data_from_mp4_track: Option<FromMp4Track>,
    ) -> Result<Self> {
        let mut tz_offset = None;
        let mut h264_metadata = None;
        let mut scratch = Vec::new();
        let mut parsing_ctx = H264ParsingContext::default();
        let mut frame0_precision_time = None;
        let mut frame_to_nalu_index = Vec::new();
        let mut last_precision_time = None;
        let mut next_frame_num = 0;

        // Use data from container if present
        if let Some(dfc) = data_from_mp4_track {
            log::debug!("Using SPS and PPS data from mp4 track.");
            {
                // SPS
                let sps_nal = RefNal::new(&dfc.sequence_parameter_set, &[], true);
                if sps_nal.header().unwrap().nal_unit_type() != UnitType::SeqParameterSet {
                    anyhow::bail!("expected SPS NAL");
                }

                let isps =
                    h264_reader::nal::sps::SeqParameterSet::from_bits(sps_nal.rbsp_bits()).unwrap();
                parsing_ctx.put_seq_param_set(isps);
            }

            {
                // PPS
                let pps_nal = RefNal::new(&dfc.picture_parameter_set, &[], true);
                if pps_nal.header().unwrap().nal_unit_type() != UnitType::PicParameterSet {
                    anyhow::bail!("expected PPS NAL");
                }

                let ipps = h264_reader::nal::pps::PicParameterSet::from_bits(
                    &parsing_ctx,
                    pps_nal.rbsp_bits(),
                )
                .unwrap();
                parsing_ctx.put_pic_param_set(ipps);
            }
        }

        // iterate through all NAL units.
        log::debug!("iterating through all NAL units");
        for (nalu_index, nal_unit) in nal_units.iter().enumerate() {
            let nal = RefNal::new(nal_unit, &[], true);
            let nal_unit_type = nal.header().unwrap().nal_unit_type();
            log::debug!("NALU index {nalu_index}, {nal_unit_type:?}");
            match nal_unit_type {
                UnitType::SEI => {
                    let mut sei_reader = SeiReader::from_rbsp_bytes(nal.rbsp_bytes(), &mut scratch);
                    loop {
                        match sei_reader.next() {
                            Ok(Some(sei_message)) => {
                                match &sei_message.payload_type {
                                    HeaderType::UserDataUnregistered => {
                                        let udu = UserDataUnregistered::read(&sei_message)?;
                                        match udu.uuid {
                                            &H264_METADATA_UUID => {
                                                let md: H264Metadata =
                                                    serde_json::from_slice(udu.payload)?;
                                                if md.version != H264_METADATA_VERSION {
                                                    anyhow::bail!(
                                                        "unexpected version in h264 metadata"
                                                    );
                                                }
                                                if h264_metadata.is_some() {
                                                    anyhow::bail!(
                                                        "multiple SEI messages, but expected exactly one"
                                                    );
                                                }

                                                tz_offset = Some(*md.creation_time.offset());
                                                h264_metadata = Some(md);
                                            }
                                            b"MISPmicrosectime" => {
                                                let precision_time =
                                                    parse_precision_time(udu.payload)
                                                        .with_context(|| {
                                                            "Parsing precision time stamp"
                                                        })?;
                                                last_precision_time = Some(precision_time);
                                                if next_frame_num == 0 {
                                                    frame0_precision_time = Some(precision_time);
                                                }
                                            }
                                            _uuid => {
                                                // anyhow::bail!("unexpected SEI UDU UUID: {uuid:?}");
                                            }
                                        }
                                    }
                                    _ => {
                                        // handle other SEI types.
                                    }
                                }
                            }
                            Ok(None) => {
                                break;
                            }
                            Err(BitReaderError::ReaderErrorFor(what, io_err)) => {
                                log::error!(
                                    "Ignoring error when reading SEI NAL unit {what}: {io_err:?}"
                                );
                                // We do not process this NAL unit but nor do we
                                // propagate the error further. FFMPEG also
                                // skips this error except writing "SEI type 5
                                // size X truncated at Y" where Y is less than
                                // X.
                            }
                            Err(e) => {
                                anyhow::bail!(
                                    "unexpected error reading NAL unit {nalu_index} SEI: {e:?}"
                                );
                            }
                        }
                    }
                }
                UnitType::SeqParameterSet => {
                    let isps =
                        h264_reader::nal::sps::SeqParameterSet::from_bits(nal.rbsp_bits()).unwrap();
                    parsing_ctx.put_seq_param_set(isps);
                }
                UnitType::PicParameterSet => {
                    match h264_reader::nal::pps::PicParameterSet::from_bits(
                        &parsing_ctx,
                        nal.rbsp_bits(),
                    ) {
                        Ok(ipps) => {
                            parsing_ctx.put_pic_param_set(ipps);
                        }
                        Err(h264_reader::nal::pps::PpsError::BadPicParamSetId(
                            h264_reader::nal::pps::ParamSetIdError::IdTooLarge(_id),
                        )) => {
                            // While this is open, ignore the error.
                            // https://github.com/dholroyd/h264-reader/issues/56
                        }
                        Err(e) => {
                            anyhow::bail!("reading PPS: {e:?}");
                        }
                    }
                }
                UnitType::SliceLayerWithoutPartitioningIdr
                | UnitType::SliceLayerWithoutPartitioningNonIdr => {
                    frame_to_nalu_index.push((nalu_index, last_precision_time));
                    last_precision_time = None;
                    next_frame_num += 1;
                }
                _nal_unit_type => {}
            }
        }

        let mut widthheight = None;
        for sps in parsing_ctx.sps() {
            if let Ok(wh) = sps.pixel_dimensions() {
                widthheight = Some(wh);
            }
        }

        let (width, height) =
            widthheight.ok_or_else(|| anyhow::anyhow!("expected SPS not found"))?;

        let timezone = tz_offset.unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());

        let frame0_precision_time = frame0_precision_time
            .as_ref()
            .map(|dt| dt.with_timezone(&timezone));

        let (timestamp_source, has_timestamps) = if frame0_precision_time.is_some() {
            ("MISPmicrosectime", true)
        } else {
            if mp4_pts.is_some() {
                ("MP4 PTS", true)
            } else {
                ("(no timestamps)", false)
            }
        };

        Ok(Self {
            nal_units,
            mp4_pts,
            frame_to_nalu_index,
            h264_metadata,
            frame0_precision_time,
            width,
            height,
            do_decode_h264,
            timestamp_source,
            has_timestamps,
        })
    }

    /// split Annex B data into NAL units.
    pub(crate) fn from_annexb<R: Read + Seek>(mut rdr: R, do_decode_h264: bool) -> Result<Self> {
        let raw_h264_buf: Vec<u8> = {
            let mut raw_h264_buf = Vec::new();
            rdr.read_to_end(&mut raw_h264_buf)?;
            raw_h264_buf
        };

        let nal_units: Vec<_> = h264_annexb_split(&raw_h264_buf).collect();
        Self::from_nal_units(nal_units, do_decode_h264, None, None)
    }
}

struct RawH264Iter<'a> {
    parent: &'a mut H264Source,
    /// frame index (not NAL unit index)
    frame_idx: usize,
    next_nal_idx: usize,
    openh264_decoder_state: Option<openh264::decoder::Decoder>,
}

impl<'a> Iterator for RawH264Iter<'a> {
    type Item = Result<FrameData>;
    fn next(&mut self) -> Option<Self::Item> {
        let frame_number = self.frame_idx;
        let res = self.parent.frame_to_nalu_index.get(self.frame_idx);
        self.frame_idx += 1;

        match res {
            None => {
                // end of frame data
                None
            }
            Some((frame_nalu_index, precise_timestamp)) => {
                // create slice of all NAL units up to frame data
                let nal_units = &self.parent.nal_units[self.next_nal_idx..=(*frame_nalu_index)];
                let mp4_pts = self.parent.mp4_pts.as_ref().map(|x| x[self.next_nal_idx]);
                let fraction_done = self.next_nal_idx as f32 / self.parent.nal_units.len() as f32;

                self.next_nal_idx = *frame_nalu_index + 1;

                let frame_timestamp = if let Some(f0) = &self.parent.frame0_precision_time {
                    Timestamp::Duration(
                        precise_timestamp
                            .unwrap()
                            .signed_duration_since(*f0)
                            .to_std()
                            .unwrap(),
                    )
                } else if let Some(mp4_pts) = mp4_pts {
                    Timestamp::Duration(mp4_pts)
                } else {
                    Timestamp::Fraction(fraction_done)
                };

                if let Some(decoder) = &mut self.openh264_decoder_state {
                    // copy into Annex B format for OpenH264
                    let annex_b = copy_nalus_to_annex_b(nal_units);

                    let decode_result = decoder.decode(&annex_b[..]);
                    match decode_result {
                        Ok(Some(decoded_yuv)) => {
                            let dim = decoded_yuv.dimension_rgb();

                            let stride = dim.0 * 3;
                            let mut image_data = vec![0u8; stride * dim.1];
                            decoded_yuv.write_rgb8(&mut image_data);

                            let host_timestamp = match precise_timestamp {
                                Some(ts) => *ts,
                                None => {
                                    if let (Some(mp4_pts), Some(md)) =
                                        (mp4_pts, &self.parent.h264_metadata)
                                    {
                                        md.creation_time.with_timezone(&chrono::Utc)
                                            + chrono::Duration::from_std(mp4_pts).unwrap()
                                    } else {
                                        // No possible source of timestamp, use dummy value.
                                        chrono::TimeZone::timestamp_opt(&chrono::Utc, 0, 0).unwrap()
                                    }
                                }
                            };

                            let extra = Box::new(basic_frame::BasicExtra {
                                host_timestamp,
                                host_framenumber: frame_number,
                            });
                            let dynamic_frame =
                                basic_frame::DynamicFrame::RGB8(basic_frame::BasicFrame::<
                                    machine_vision_formats::pixel_format::RGB8,
                                > {
                                    width: dim.0.try_into().unwrap(),
                                    height: dim.1.try_into().unwrap(),
                                    stride: u32::try_from(stride).unwrap(),
                                    image_data,
                                    pixel_format: std::marker::PhantomData,
                                    extra,
                                });

                            let buf_len = nal_units.iter().map(|x| x.len()).sum();
                            // let buf_len = avcc_data.len();
                            let idx = frame_number;
                            let image = ImageData::Decoded(dynamic_frame);
                            Some(Ok(FrameData {
                                timestamp: frame_timestamp,
                                image,
                                buf_len,
                                idx,
                            }))

                            // Some(ImageData::Decoded(dynamic_frame))
                        }
                        Ok(None) => Some(Err(anyhow::anyhow!(
                            "decoder unexpectedly did not return image data"
                        ))),
                        Err(decode_err) => Some(Err(decode_err.into())),
                    }
                } else {
                    let buf_len = nal_units.iter().map(|x| x.len()).sum();
                    // let buf_len = avcc_data.len();
                    let idx = frame_number;
                    let buf = EncodedH264 {
                        data: H264EncodingVariant::RawEbsp(nal_units.to_vec()),
                        has_precision_timestamp: self.parent.frame0_precision_time.is_some(),
                    };
                    let image = ImageData::EncodedH264(buf);
                    Some(Ok(FrameData {
                        timestamp: frame_timestamp,
                        image,
                        buf_len,
                        idx,
                    }))
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.parent.frame_to_nalu_index.len() - self.frame_idx;
        (remaining, Some(remaining))
    }
}

pub fn from_annexb_path<P: AsRef<Path>>(path: P, do_decode_h264: bool) -> Result<H264Source> {
    let rdr = std::fs::File::open(path.as_ref())
        .with_context(|| format!("Opening {}", path.as_ref().display()))?;
    from_annexb_reader(rdr, do_decode_h264)
        .with_context(|| format!("Reading H264 file {}", path.as_ref().display()))
}

pub fn from_annexb_reader<R: Read + Seek>(rdr: R, do_decode_h264: bool) -> Result<H264Source> {
    let buf_reader = BufReader::new(rdr);
    H264Source::from_annexb(buf_reader, do_decode_h264)
}

pub(crate) struct UserDataUnregistered<'a> {
    pub uuid: &'a [u8; 16],
    pub payload: &'a [u8],
}

impl<'a> UserDataUnregistered<'a> {
    pub fn read(msg: &SeiMessage<'a>) -> Result<UserDataUnregistered<'a>> {
        if msg.payload_type != HeaderType::UserDataUnregistered {
            anyhow::bail!(
                "expected UserDataUnregistered message, found {:?}",
                msg.payload_type
            );
        }
        if msg.payload.len() < 16 {
            anyhow::bail!("SEI payload too short to contain UserDataUnregistered message");
        }
        let uuid = (&msg.payload[0..16]).try_into().unwrap();

        let payload = &msg.payload[16..];
        Ok(UserDataUnregistered { uuid, payload })
    }
}

pub(crate) fn parse_precision_time(payload: &[u8]) -> Result<chrono::DateTime<chrono::Utc>> {
    if payload.len() != 12 {
        anyhow::bail!("unexpected payload length");
    }

    // // Time Stamp Status byte from MISB Standard 0603.
    // // Could parse Locked/Unlocked (bit 7), Normal/Discontinuity (bit 6),
    // // Forward/Reverse (bit 5).

    // let time_stamp_status = payload[0];
    // if time_stamp_status & 0x1F != 0x1F {
    //     anyhow::bail!(
    //         "unexpected time stamp status byte. Full payload: {{{}}}",
    //         pretty_hex::simple_hex(&payload),
    //     );
    // }

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

/// Copy raw headerless EBSP NAL units to Annex B
fn copy_nalus_to_annex_b(nalus: &[Vec<u8>]) -> Vec<u8> {
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
