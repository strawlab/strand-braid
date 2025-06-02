// Copyright 2022-2024 Andrew D. Straw.
use std::{
    io::{BufReader, Read, Seek},
    path::Path,
};

use chrono::{DateTime, FixedOffset, Utc};
use h264_reader::{
    nal::{
        sei::{HeaderType, SeiMessage, SeiReader},
        Nal, RefNal, UnitType,
    },
    rbsp::BitReaderError,
    Context as H264ParsingContext,
};
use serde::{Deserialize, Serialize};

use strand_cam_remote_control::{H264Metadata, H264_METADATA_UUID, H264_METADATA_VERSION};

#[cfg(feature = "openh264")]
use machine_vision_formats::owned::OImage;

use crate::{
    ntp_timestamp::NtpTimestamp,
    srt_reader::{self, Stanza},
    EncodedH264, Error, FrameData, FrameDataSource, H264EncodingVariant, ImageData, MyAsStr,
    Result, Timestamp, TimestampSource,
};

struct SrtData {
    stanzas: Vec<Stanza>,
    frame0_time: DateTime<FixedOffset>,
    idx: usize,
}

#[derive(serde::Deserialize)]
struct SrtMsg {
    timestamp: DateTime<chrono::FixedOffset>,
}

impl SrtData {
    fn parse_time(stanza: &Stanza) -> DateTime<FixedOffset> {
        let msg: SrtMsg = serde_json::from_str(&stanza.lines).unwrap();
        msg.timestamp
    }
    fn next_pts(&mut self) -> Result<std::time::Duration> {
        let stanza = &self.stanzas[self.idx];
        self.idx += 1;
        let tnow = Self::parse_time(stanza);
        Ok(tnow.signed_duration_since(self.frame0_time).to_std()?)
    }
    fn frame0_time(&self) -> DateTime<FixedOffset> {
        self.frame0_time
    }
}

pub trait H264Preparser {
    fn put_seq_param_set(&mut self, nalu: &RefNal<'_>) -> eyre::Result<()>;
    fn put_pic_param_set(&mut self, nalu: &RefNal<'_>) -> eyre::Result<()>;
    fn put_sei_nalu(&mut self, nalu: &RefNal<'_>) -> eyre::Result<()>;
    fn put_slice_layer_nalu(&mut self, nalu: &RefNal<'_>, is_i_frame: bool) -> eyre::Result<()>;
    fn set_num_positions(&mut self, num_positions: usize) -> eyre::Result<()>;
    fn set_position(&mut self, pos: usize) -> eyre::Result<()>;
    fn close(self) -> eyre::Result<()>;
}

// Found in libx264-encoded h264 streams. See
// https://code.videolan.org/videolan/x264/-/blob/da14df5535/encoder/set.c#L598
const X264_UUID: &[u8; 16] = uuid::uuid!("dc45e9bd-e6d9-48b7-962c-d820d923eeef").as_bytes();

// Found in videotoolbox-encoded h264 streams.
const VIDEOTOOLBOX_UUID: &[u8; 16] = uuid::uuid!("47564adc-5c4c-433f-94ef-c5113cd143a8").as_bytes();

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
/// (0, 1, 2 or more) H264 NAL units. A sample can contain zero, one, two or
/// more image frames of data. (There exist MP4 files in which a sample contains
/// zero NAL units and thus zero image frames as well as MP4 files in which a
/// single sample contains many NAL units and many image frames.) Samples also
/// carry a duration, which is informational to assist with playback. The
/// duration of frame N should be the PTS of frame N+1 - the PTS of frame N.
/// There seems to be a general assumption that samples should be equi-distant
/// in time and thus that the file has a constant frame rate, although I have
/// not found this in any specification.
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
pub struct H264Source<H: SeekableH264Source> {
    seekable_h264_source: H,
    /// For every NAL unit, the coordinates in the source to read it.
    nal_locations: Vec<H::NalLocation>,
    /// timestamps from MP4 files, one per MP4 sample (which we assume to be one per frame)
    mp4_pts: Option<Vec<std::time::Duration>>,
    frame_time_info: Vec<FrameTimeInfo>,
    pub h264_metadata: Option<H264Metadata>,
    frame0_precision_time: Option<chrono::DateTime<chrono::FixedOffset>>,
    frame0_frameinfo: Option<FrameInfo>,
    width: u32,
    height: u32,
    do_decode_h264: bool,
    timestamp_source: Option<crate::TimestampSource>,
    has_timestamps: bool,
    srt_data: Option<SrtData>,
    average_fps: Option<f64>,
}

impl<H: SeekableH264Source> H264Source<H> {
    pub fn as_seekable_h264_source(&self) -> &H {
        &self.seekable_h264_source
    }

    fn create_iter_unchecked<'a>(
        &'a mut self,
        frame_idx: usize,
    ) -> Box<dyn Iterator<Item = Result<FrameData>> + 'a> {
        let openh264_decoder_state = if self.do_decode_h264 {
            Some(crate::opt_openh264_decoder::DecoderType::new().unwrap())
        } else {
            None
        };
        Box::new(RawH264Iter {
            parent: self,
            frame_idx,
            openh264_decoder_state,
        })
    }
}

/// Timing information for a frame of video.
pub struct FrameTimeInfo {
    /// The NAL unit location.
    ///
    /// This is an index into the slice &[SeekableH264Source::NalLocation]
    /// returned by [SeekableH264Source::nal_boundaries]. In the case of MP4
    /// files, each index corresponds to multiple NAL units.
    nal_location_index: usize,
    precise_timestamp: Option<DateTime<Utc>>,
    frameinfo: Option<FrameInfo>,
}

impl<H: SeekableH264Source> FrameDataSource for H264Source<H> {
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
        match &self.timestamp_source {
            Some(TimestampSource::BestGuess) => unreachable!(),
            Some(TimestampSource::FixedFramerate) => {
                if let Some(t) = &self.frame0_precision_time {
                    Some(*t)
                } else if let Some(fi) = &self.frame0_frameinfo {
                    Some(fi.recv.into())
                } else {
                    None
                }
            }
            Some(TimestampSource::MispMicrosectime) => self.frame0_precision_time,
            Some(TimestampSource::FrameInfoRecvTime) | Some(TimestampSource::FrameInfoRtp) => {
                Some(self.frame0_frameinfo.as_ref().unwrap().recv.into())
            }
            Some(TimestampSource::Mp4Pts) | None => None,
            Some(TimestampSource::SrtFile) => self.srt_data.as_ref().map(|x| x.frame0_time()),
        }
    }
    fn average_framerate(&self) -> Option<f64> {
        self.average_fps
    }
    fn skip_n_frames(&mut self, n_frames: usize) -> Result<()> {
        if n_frames > 0 {
            return Err(Error::SkippingFramesNotSupported);
            // Doing so would require finding I frames and only skipping to
            // those (or decoding and interpolating a new I frame).
            // Also: caching SPS and PPS would be required.
            // We do this in the MKV reader, so we should use that
            // implementation for inspiration.
        }
        Ok(())
    }
    fn estimate_luminance_range(&mut self) -> Result<(u16, u16)> {
        Err(Error::NotImplemented("h264 luminance scanning"))
    }
    fn iter<'a>(&'a mut self) -> Box<dyn Iterator<Item = Result<FrameData>> + 'a> {
        self.create_iter_unchecked(0)
    }
    fn timestamp_source(&self) -> &str {
        self.timestamp_source.as_str()
    }
    fn has_timestamps(&self) -> bool {
        self.has_timestamps
    }
}

pub(crate) struct FromMp4Track {
    pub(crate) sequence_parameter_set: Vec<u8>,
    pub(crate) picture_parameter_set: Vec<u8>,
}

pub trait SeekRead: Seek + Read {}
impl<T> SeekRead for T where T: Seek + Read {}

pub trait SeekableH264Source {
    type NalLocation;
    fn nal_boundaries(&mut self) -> &[Self::NalLocation];
    /// Read multiple NAL units at the specified location
    fn read_nal_units_at_location(&mut self, location: &Self::NalLocation) -> Result<Vec<Vec<u8>>>;
    /// Read multiple NAL units at multiple specified locations
    fn read_nal_units_at_locations(
        &mut self,
        locations: &[Self::NalLocation],
    ) -> Result<Vec<Vec<u8>>> {
        let mut result = Vec::with_capacity(locations.len() * 3);
        for location in locations.iter() {
            let nal_units = self.read_nal_units_at_location(location)?;
            result.extend(nal_units);
        }
        Ok(result)
    }

    /// Return the first SPS
    fn first_sps(&self) -> Option<Vec<u8>>;
    /// Return the first PPS
    fn first_pps(&self) -> Option<Vec<u8>>;
}

#[derive(Debug, PartialEq, Clone)]
pub struct AnnexBLocation {
    pub(crate) start: u64,
    pub(crate) sz: usize,
}

pub struct H264AnnexBSource {
    inner: Box<dyn SeekRead + Send>,
    my_nal_boundaries: Vec<AnnexBLocation>,
}

impl H264AnnexBSource {
    pub fn from_file(fd: std::fs::File) -> Result<Self> {
        let inner = Box::new(BufReader::new(fd));
        Self::from_readseek(inner)
    }
    pub fn from_readseek(mut inner: Box<(dyn SeekRead + Send)>) -> Result<Self> {
        inner.seek(std::io::SeekFrom::Start(0))?;
        let my_nal_boundaries = crate::h264_annexb_splitter::find_nals(&mut inner)?;
        inner.seek(std::io::SeekFrom::Start(0))?;
        Ok(Self {
            inner,
            my_nal_boundaries,
        })
    }
}

impl SeekableH264Source for H264AnnexBSource {
    type NalLocation = AnnexBLocation;
    fn nal_boundaries(&mut self) -> &[Self::NalLocation] {
        &self.my_nal_boundaries
    }
    fn read_nal_units_at_location(&mut self, location: &Self::NalLocation) -> Result<Vec<Vec<u8>>> {
        self.inner.seek(std::io::SeekFrom::Start(location.start))?;
        let mut buf = vec![0u8; location.sz];
        self.inner.read_exact(&mut buf)?;
        Ok(vec![buf])
    }

    fn first_sps(&self) -> Option<Vec<u8>> {
        None
    }
    fn first_pps(&self) -> Option<Vec<u8>> {
        None
    }
}

impl<H> H264Source<H>
where
    H: SeekableH264Source,
    <H as SeekableH264Source>::NalLocation: Clone,
{
    pub(crate) fn from_seekable_h264_source_with_timestamp_source(
        mut seekable_h264_source: H,
        do_decode_h264: bool,
        mp4_pts: Option<Vec<std::time::Duration>>,
        data_from_mp4_track: Option<FromMp4Track>,
        timestamp_source: crate::TimestampSource,
        srt_file_path: Option<std::path::PathBuf>,
        show_progress: bool,
        mut preparser: Option<Box<dyn H264Preparser>>,
    ) -> Result<Self> {
        let nal_locations: Vec<H::NalLocation> = seekable_h264_source.nal_boundaries().to_vec();

        let mut parsing_ctx = H264ParsingContext::default();

        // open SRT file
        if timestamp_source == crate::TimestampSource::SrtFile && srt_file_path.is_none() {
            return Err(Error::NoSrtPathGiven);
        }

        let srt_data = if let Some(srt_file_path) = srt_file_path {
            let stanzas = srt_reader::read_srt_file(&srt_file_path)?;
            let frame0_time = SrtData::parse_time(&stanzas[0]);
            Some(SrtData {
                stanzas,
                idx: 0,
                frame0_time,
            })
        } else {
            None
        };

        // Use data from container if present
        if let Some(dfc) = data_from_mp4_track {
            tracing::debug!("Using SPS and PPS data from mp4 track.");
            {
                // SPS
                let sps_nal = RefNal::new(&dfc.sequence_parameter_set, &[], true);
                if sps_nal.header().unwrap().nal_unit_type() != UnitType::SeqParameterSet {
                    return Err(Error::ExpectedSpsNotFound);
                }

                let isps =
                    h264_reader::nal::sps::SeqParameterSet::from_bits(sps_nal.rbsp_bits()).unwrap();
                if let Some(preparser) = preparser.as_mut() {
                    preparser
                        .put_seq_param_set(&sps_nal)
                        .map_err(Error::PreParserError)?;
                }
                parsing_ctx.put_seq_param_set(isps);
            }

            {
                // PPS
                let pps_nal = RefNal::new(&dfc.picture_parameter_set, &[], true);
                if pps_nal.header().unwrap().nal_unit_type() != UnitType::PicParameterSet {
                    return Err(Error::ExpectedPpsNotFound);
                }

                let ipps = h264_reader::nal::pps::PicParameterSet::from_bits(
                    &parsing_ctx,
                    pps_nal.rbsp_bits(),
                )
                .unwrap();
                if let Some(preparser) = preparser.as_mut() {
                    preparser
                        .put_pic_param_set(&pps_nal)
                        .map_err(Error::PreParserError)?;
                }
                parsing_ctx.put_pic_param_set(ipps);
            }
        }

        // iterate through all NAL units.
        let (frame_time_info, frame0_precision_time, frame0_frameinfo, h264_metadata, tz_offset) =
            load_timing_data(
                &nal_locations,
                &mut seekable_h264_source,
                &mut parsing_ctx,
                show_progress,
                preparser,
            )?;

        let mut widthheight = None;
        for sps in parsing_ctx.sps() {
            if let Ok(wh) = sps.pixel_dimensions() {
                widthheight = Some(wh);
            }
        }

        let (width, height) = widthheight.ok_or_else(|| crate::Error::ExpectedSpsNotFound)?;

        let timezone = tz_offset.unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());

        let frame0_precision_time = frame0_precision_time
            .as_ref()
            .map(|dt| dt.with_timezone(&timezone));

        let (timestamp_source, has_timestamps) = match timestamp_source {
            crate::TimestampSource::BestGuess => {
                if frame0_precision_time.is_some() {
                    (Some(crate::TimestampSource::MispMicrosectime), true)
                } else if frame0_frameinfo.is_some() {
                    (Some(crate::TimestampSource::FrameInfoRtp), true)
                } else if mp4_pts.is_some() {
                    (Some(crate::TimestampSource::Mp4Pts), true)
                } else {
                    (None, false)
                }
            }
            crate::TimestampSource::FixedFramerate => (Some(timestamp_source), true),
            crate::TimestampSource::FrameInfoRecvTime | crate::TimestampSource::FrameInfoRtp => {
                if frame0_frameinfo.is_none() {
                    return Err(Error::H264TimestampError(
                        "Requested timestamp that requires FrameInfo, but this information is not present."
                            .into(),
                    ));
                }
                (Some(timestamp_source), true)
            }
            crate::TimestampSource::MispMicrosectime => {
                if frame0_precision_time.is_none() {
                    return Err(Error::H264TimestampError(
                        "Requested timestamp source MispMicrosectime, but frame0_precision_time not present."
                            .into(),
                    ));
                }
                (Some(timestamp_source), true)
            }
            crate::TimestampSource::Mp4Pts => {
                if mp4_pts.is_none() {
                    return Err(Error::H264TimestampError(
                        "Requested timestamp source Mp4Pts, but MP4 PTS not present.".into(),
                    ));
                }
                (Some(timestamp_source), true)
            }
            crate::TimestampSource::SrtFile => (Some(timestamp_source), true),
        };

        if let Some(mp4_pts) = mp4_pts.as_ref() {
            if mp4_pts.len() != frame_time_info.len() {
                return Err(Error::H264TimestampError(format!(
                    "We have {} frames of MP4 PTS timing, but computed {} frames of video.",
                    mp4_pts.len(),
                    frame_time_info.len()
                )));
            }
        }
        let average_fps = calc_avg_fps(&frame_time_info[..]);

        Ok(Self {
            seekable_h264_source,
            nal_locations,
            mp4_pts,
            frame_time_info,
            h264_metadata,
            frame0_precision_time,
            frame0_frameinfo,
            width,
            height,
            do_decode_h264,
            timestamp_source,
            has_timestamps,
            srt_data,
            average_fps,
        })
    }
}

fn calc_avg_fps(fti: &[FrameTimeInfo]) -> Option<f64> {
    if fti.len() <= 1 {
        return None;
    }
    let frames = (fti.len() - 1) as f64;
    if let Some(t0) = fti[0].precise_timestamp {
        // prefer precise_timestamps
        let tend = fti[fti.len() - 1].precise_timestamp.unwrap();
        let secs = (tend - t0).to_std().unwrap().as_secs_f64();
        Some(frames / secs)
    } else if let Some(fi) = &fti[0].frameinfo {
        // else use FrameInfo
        let t0: chrono::DateTime<chrono::Utc> = fi.recv.into();
        let tend: chrono::DateTime<chrono::Utc> =
            fti[fti.len() - 1].frameinfo.as_ref().unwrap().recv.into();
        let secs = (tend - t0).to_std().unwrap().as_secs_f64();
        Some(frames / secs)
    } else {
        // final resort
        None
    }
}

fn load_timing_data<H>(
    nal_locations: &[H::NalLocation],
    seekable_h264_source: &mut H,
    parsing_ctx: &mut H264ParsingContext,
    show_progress: bool,
    mut preparser: Option<Box<dyn H264Preparser>>,
) -> Result<(
    Vec<FrameTimeInfo>,
    Option<DateTime<Utc>>,
    Option<FrameInfo>,
    Option<H264Metadata>,
    Option<FixedOffset>,
)>
where
    H: SeekableH264Source,
    <H as SeekableH264Source>::NalLocation: Clone,
{
    let mut scratch = Vec::new();

    let mut tz_offset = None;

    let mut h264_metadata = None;

    // One entry per frame. Can refer to multiple multiple NAL units, e.g.
    // in MP4 files where a frame is an MP4 sample containing multiple NAL
    // units.
    let mut frame_time_info = Vec::new();

    let mut frame0_precision_time = None;
    let mut frame0_frameinfo = None;

    tracing::debug!(
        "Iterating through NAL units at {} locations to load timing data.",
        nal_locations.len()
    );

    let mut pb = if show_progress {
        // Custom progress bar with space at right end to prevent obscuring last
        // digit with cursor.
        let style = indicatif::ProgressStyle::with_template(
            "Iterating NAL units in h264 source {wide_bar} {pos}/{len} ETA: {eta} ",
        )
        .unwrap();
        Some(indicatif::ProgressBar::new(nal_locations.len().try_into().unwrap()).with_style(style))
    } else {
        None
    };

    if let Some(preparser) = preparser.as_mut() {
        preparser
            .set_num_positions(nal_locations.len())
            .map_err(Error::PreParserError)?;
    }
    // Cached value of MISP time data for the frame whose data is being accumulated.
    let mut precise_timestamp = None;
    // Cached value of frame number as we accumluate data.
    let mut next_frame_num = 0;

    // Cached value of FrameInfo time data for the frame whose data is being
    // accumulated.
    let mut frameinfo = None;

    for (nal_location_index, nal_location) in nal_locations.iter().enumerate() {
        if let Some(preparser) = preparser.as_mut() {
            preparser
                .set_position(nal_location_index)
                .map_err(Error::PreParserError)?;
        }

        if let Some(pb) = pb.as_mut() {
            pb.set_position(nal_location_index.try_into().unwrap());
        }

        // Read all NAL units from this location. (For MP4 files, this means
        // read all NAL units from this sample. For H264 AnnexB files, this
        // will read a single NAL unit.)
        let nal_units = seekable_h264_source.read_nal_units_at_location(nal_location)?;
        for nal_unit in nal_units.iter() {
            // Note, there are multiple NAL units per `nal_location_index`
            // in MP4 files because in that case, `nal_location_index`
            // refers to the MP4 sample which has multiple NAL units.
            let nal = RefNal::new(nal_unit.as_slice(), &[], true);
            let nal_unit_type = nal.header().unwrap().nal_unit_type();
            tracing::trace!("NAL unit location index {nal_location_index}, {nal_unit_type:?}");
            match nal_unit_type {
                UnitType::SEI => {
                    if let Some(preparser) = preparser.as_mut() {
                        preparser
                            .put_sei_nalu(&nal)
                            .map_err(Error::PreParserError)?;
                    }
                    let mut sei_reader = SeiReader::from_rbsp_bytes(nal.rbsp_bytes(), &mut scratch);
                    loop {
                        match sei_reader.next() {
                            Ok(Some(sei_message)) => {
                                tracing::trace!("SEI payload type: {:?}", sei_message.payload_type);
                                match &sei_message.payload_type {
                                    HeaderType::UserDataUnregistered => {
                                        let udu = UserDataUnregistered::read(&sei_message)?;
                                        match udu.uuid {
                                            &H264_METADATA_UUID => {
                                                let md: H264Metadata =
                                                    serde_json::from_slice(udu.payload)?;
                                                if md.version != H264_METADATA_VERSION {
                                                    return Err(Error::H264Error(
                                                        "unexpected version in h264 metadata",
                                                    ));
                                                }
                                                if h264_metadata.is_some() {
                                                    return Err(Error::H264Error(
                                                        "multiple SEI messages, but expected exactly one"
                                                    ));
                                                }

                                                tracing::debug!("Found H264_METADATA_UUID: {md:?}");
                                                tz_offset = Some(*md.creation_time.offset());
                                                h264_metadata = Some(md);
                                            }
                                            X264_UUID => {
                                                let payload_str =
                                                    String::from_utf8_lossy(udu.payload);
                                                tracing::trace!(
                                                    "Ignoring SEI UserDataUnregistered x264 payload: {}",
                                                    payload_str,
                                                );
                                            }
                                            VIDEOTOOLBOX_UUID => {
                                                tracing::trace!(
                                                "Ignoring SEI UserDataUnregistered from videotoolbox."
                                            );
                                            }
                                            b"MISPmicrosectime" => {
                                                let ts = parse_precision_time(udu.payload)?;
                                                tracing::trace!("Found MISPmicrosectime: {ts:?}");
                                                precise_timestamp = Some(ts);
                                                if next_frame_num == 0 {
                                                    frame0_precision_time = Some(ts);
                                                }
                                            }
                                            b"strawlab.org/89H" => {
                                                let fi: FrameInfo =
                                                    serde_json::from_slice(udu.payload)?;
                                                tracing::trace!("Found 89H FrameInfo: {fi:?}");
                                                frameinfo = Some(fi.clone());
                                                if next_frame_num == 0 {
                                                    frame0_frameinfo = Some(fi);
                                                }
                                            }
                                            _uuid => {
                                                tracing::trace!(
                                                    "Ignoring SEI UserDataUnregistered uuid: {}",
                                                    uuid::Uuid::from_bytes(*udu.uuid).to_string(),
                                                );
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
                                tracing::error!(
                                    "Ignoring error when reading SEI NAL unit {what}: {io_err:?}"
                                );
                                // We do not process this NAL unit but nor do we
                                // propagate the error further. FFMPEG also
                                // skips this error except writing "SEI type 5
                                // size X truncated at Y" where Y is less than
                                // X.
                            }
                            Err(e) => {
                                return Err(Error::H264Nal {
                                    nal_location_index,
                                    e,
                                });
                            }
                        }
                    }
                }
                UnitType::SeqParameterSet => {
                    let isps =
                        h264_reader::nal::sps::SeqParameterSet::from_bits(nal.rbsp_bits()).unwrap();
                    if let Some(preparser) = preparser.as_mut() {
                        preparser
                            .put_seq_param_set(&nal)
                            .map_err(Error::PreParserError)?;
                    }
                    parsing_ctx.put_seq_param_set(isps);
                }
                UnitType::PicParameterSet => {
                    match h264_reader::nal::pps::PicParameterSet::from_bits(
                        &parsing_ctx,
                        nal.rbsp_bits(),
                    ) {
                        Ok(ipps) => {
                            if let Some(preparser) = preparser.as_mut() {
                                preparser
                                    .put_pic_param_set(&nal)
                                    .map_err(Error::PreParserError)?;
                            }
                            parsing_ctx.put_pic_param_set(ipps);
                        }
                        Err(h264_reader::nal::pps::PpsError::BadPicParamSetId(
                            h264_reader::nal::pps::PicParamSetIdError::IdTooLarge(_id),
                        )) => {
                            // While this is open, ignore the error.
                            // https://github.com/dholroyd/h264-reader/issues/56
                        }
                        Err(e) => {
                            return Err(Error::H264Pps(format!("reading PPS: {e:?}")));
                        }
                    }
                }
                UnitType::SliceLayerWithoutPartitioningIdr
                | UnitType::SliceLayerWithoutPartitioningNonIdr => {
                    let is_i_frame = nal_unit_type == UnitType::SliceLayerWithoutPartitioningIdr;
                    if let Some(preparser) = preparser.as_mut() {
                        preparser
                            .put_slice_layer_nalu(&nal, is_i_frame)
                            .map_err(Error::PreParserError)?;
                    }
                    // The NAL unit with the video frames comes after the
                    // timing into NAL unit(s) so we gather them now.
                    frame_time_info.push(FrameTimeInfo {
                        nal_location_index,
                        precise_timestamp,
                        frameinfo,
                    });
                    // Reset temporary values.
                    precise_timestamp = None;
                    frameinfo = None;
                    next_frame_num += 1;
                }
                _nal_unit_type => {}
            }
        }
    }

    if let Some(pb) = pb.as_mut() {
        pb.finish_and_clear();
    }

    tracing::debug!("Done iterating through all NAL units.");

    Ok((
        frame_time_info,
        frame0_precision_time,
        frame0_frameinfo,
        h264_metadata,
        tz_offset,
    ))
}

struct RawH264Iter<'parent, H: SeekableH264Source> {
    parent: &'parent mut H264Source<H>,
    /// frame index (not NAL unit index)
    frame_idx: usize,
    openh264_decoder_state: Option<crate::opt_openh264_decoder::DecoderType>,
}

impl<H: SeekableH264Source> Iterator for RawH264Iter<'_, H> {
    type Item = Result<FrameData>;
    fn next(&mut self) -> Option<Self::Item> {
        let frame_number = self.frame_idx;
        let res = self.parent.frame_time_info.get(self.frame_idx);
        self.frame_idx += 1;

        res.map(|nti| {
            // create slice of all NAL units up and including NALU for the frame
            let nal_locations = &self.parent.nal_locations[frame_number..=(nti.nal_location_index)];
            let mp4_pts = self.parent.mp4_pts.as_ref().map(|x| x[frame_number]); // one per mp4 sample
            let fraction_done = frame_number as f32 / self.parent.nal_locations.len() as f32;

            let frame_timestamp = match self.parent.timestamp_source {
                Some(TimestampSource::BestGuess) => unreachable!(),
                Some(TimestampSource::MispMicrosectime) => {
                    let f0 = self.parent.frame0_precision_time.as_ref().unwrap();
                    Timestamp::Duration(
                        nti.precise_timestamp
                            .unwrap()
                            .signed_duration_since(*f0)
                            .to_std()
                            .unwrap(),
                    )
                }
                Some(TimestampSource::FrameInfoRecvTime) => {
                    let t0 = self.parent.frame0_frameinfo.as_ref().unwrap().recv;
                    let t0: chrono::DateTime<chrono::Utc> = t0.into();
                    let this_frame: chrono::DateTime<chrono::Utc> =
                        nti.frameinfo.as_ref().unwrap().recv.into();
                    Timestamp::Duration(this_frame.signed_duration_since(t0).to_std().unwrap())
                }
                Some(TimestampSource::FrameInfoRtp) => {
                    let fi0 = self.parent.frame0_frameinfo.as_ref().unwrap();
                    let rtp0 = fi0.rtp;
                    let rtp_now = nti.frameinfo.as_ref().unwrap().rtp;
                    let rtp_dur = rtp_now.wrapping_sub(rtp0);
                    let rtp_dur_secs = rtp_dur as f64 / 90000.0; // nominally 90 kHz
                    Timestamp::Duration(std::time::Duration::from_secs_f64(rtp_dur_secs))
                }
                Some(TimestampSource::FixedFramerate) => {
                    let dur_secs = nti.nal_location_index as f64 / self.parent.average_fps.unwrap();
                    Timestamp::Duration(std::time::Duration::from_secs_f64(dur_secs))
                }
                Some(TimestampSource::Mp4Pts) => Timestamp::Duration(mp4_pts.unwrap()),
                Some(TimestampSource::SrtFile) => {
                    let srt_data = self.parent.srt_data.as_mut().unwrap();
                    let pts = srt_data.next_pts().unwrap();
                    Timestamp::Duration(pts)
                }
                None => Timestamp::Fraction(fraction_done),
            };

            let nal_units = self
                .parent
                .seekable_h264_source
                .read_nal_units_at_locations(nal_locations)?;

            if let Some(decoder) = &mut self.openh264_decoder_state {
                // copy into Annex B format for OpenH264
                let annex_b = copy_nalus_to_annex_b(nal_units.as_slice());

                match decoder.decode(&annex_b[..])? {
                    Some(decoded_yuv) => {
                        yuv2rgb(decoded_yuv, frame_number, nal_units, frame_timestamp)
                    }
                    None => Err(crate::Error::DecoderDidNotReturnImageData),
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
                Ok(FrameData {
                    timestamp: frame_timestamp,
                    image,
                    buf_len,
                    idx,
                })
            }
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.parent.frame_time_info.len() - self.frame_idx;
        (remaining, Some(remaining))
    }
}

#[cfg(not(feature = "openh264"))]
fn yuv2rgb(
    _decoded_yuv: (),
    _frame_number: usize,
    _nal_units: Vec<Vec<u8>>,
    _timestamp: Timestamp,
) -> Result<FrameData> {
    Err(Error::H264Error("No H264 decoder support at compile time"))
}

#[cfg(feature = "openh264")]
fn yuv2rgb(
    decoded_yuv: openh264::decoder::DecodedYUV<'_>,
    frame_number: usize,
    nal_units: Vec<Vec<u8>>,
    timestamp: Timestamp,
) -> Result<FrameData> {
    use openh264::formats::YUVSource;
    let dim = decoded_yuv.dimensions();

    let stride = dim.0 * 3;
    let mut image_data = vec![0u8; stride * dim.1];
    decoded_yuv.write_rgb8(&mut image_data);

    let dynamic_frame = strand_dynamic_frame::DynamicFrameOwned::from_static(
        OImage::<machine_vision_formats::pixel_format::RGB8>::new(
            dim.0.try_into().unwrap(),
            dim.1.try_into().unwrap(),
            stride,
            image_data,
        )
        .unwrap(),
    );

    let buf_len = nal_units.iter().map(|x| x.len()).sum();
    // let buf_len = avcc_data.len();
    let idx = frame_number;
    let image = ImageData::Decoded(dynamic_frame);
    Ok(FrameData {
        timestamp,
        image,
        buf_len,
        idx,
    })
}

pub(crate) fn from_annexb_path_with_timestamp_source<P: AsRef<Path>>(
    path: P,
    do_decode_h264: bool,
    timestamp_source: crate::TimestampSource,
    srt_file_path: Option<std::path::PathBuf>,
    show_progress: bool,
) -> Result<H264Source<H264AnnexBSource>> {
    let rdr = std::fs::File::open(path.as_ref())?;
    let seekable_h264_source = H264AnnexBSource::from_file(rdr)?;
    from_annexb_reader_with_timestamp_source(
        seekable_h264_source,
        do_decode_h264,
        timestamp_source,
        srt_file_path,
        show_progress,
    )
}

fn from_annexb_reader_with_timestamp_source(
    annex_b_source: H264AnnexBSource,
    do_decode_h264: bool,
    timestamp_source: crate::TimestampSource,
    srt_file_path: Option<std::path::PathBuf>,
    show_progress: bool,
) -> Result<H264Source<H264AnnexBSource>> {
    H264Source::from_seekable_h264_source_with_timestamp_source(
        annex_b_source,
        do_decode_h264,
        None,
        None,
        timestamp_source,
        srt_file_path,
        show_progress,
        None,
    )
}

pub(crate) struct UserDataUnregistered<'a> {
    pub uuid: &'a [u8; 16],
    pub payload: &'a [u8],
}

impl<'a> UserDataUnregistered<'a> {
    pub fn read(msg: &SeiMessage<'a>) -> Result<UserDataUnregistered<'a>> {
        if msg.payload_type != HeaderType::UserDataUnregistered {
            return Err(Error::UduError(format!(
                "expected UserDataUnregistered message, found {:?}",
                msg.payload_type
            )));
        }
        if msg.payload.len() < 16 {
            return Err(Error::UduError(
                "SEI payload too short to contain UserDataUnregistered message".to_string(),
            ));
        }
        let uuid = (&msg.payload[0..16]).try_into().unwrap();

        let payload = &msg.payload[16..];
        Ok(UserDataUnregistered { uuid, payload })
    }
}

pub(crate) fn parse_precision_time(payload: &[u8]) -> Result<chrono::DateTime<chrono::Utc>> {
    if payload.len() != 12 {
        return Err(Error::UnexpectedPayloadLength);
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
            return Err(Error::UnexpectedStartCodeByte);
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

/// Timing information associated with each video frame
///
/// UUID strawlab.org/89H
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FrameInfo {
    /// Receive timestamp as NTP (Network Time Protocol) timestamp
    recv: NtpTimestamp,
    /// RTP (Real Time Protocol) timestamp as reported by the sender
    rtp: u32,
}

// ----

#[cfg(test)]
mod test {
    #[cfg(feature = "openh264")]
    #[test]
    fn parse_h264() -> crate::Result<()> {
        use super::*;

        {
            let file_buf = include_bytes!("test-data/test_less-avc_mono8_15x14.h264");
            let cursor = std::io::Cursor::new(file_buf);
            let seekable_h264_source = H264AnnexBSource::from_readseek(Box::new(cursor))?;

            let do_decode_h264 = true;
            let mut h264_src = from_annexb_reader_with_timestamp_source(
                seekable_h264_source,
                do_decode_h264,
                TimestampSource::BestGuess,
                None,
                false,
            )?;
            assert_eq!(h264_src.width(), 15);
            assert_eq!(h264_src.height(), 14);
            let frames: Vec<_> = h264_src.iter().collect();
            assert_eq!(frames.len(), 1);
        }

        {
            let file_buf = include_bytes!("test-data/test_less-avc_rgb8_16x16.h264");
            let cursor = std::io::Cursor::new(file_buf);
            let seekable_h264_source = H264AnnexBSource::from_readseek(Box::new(cursor))?;
            let do_decode_h264 = true;
            let mut h264_src = from_annexb_reader_with_timestamp_source(
                seekable_h264_source,
                do_decode_h264,
                TimestampSource::BestGuess,
                None,
                false,
            )?;
            assert_eq!(h264_src.width(), 16);
            assert_eq!(h264_src.height(), 16);
            let frames: Vec<_> = h264_src.iter().collect();
            assert_eq!(frames.len(), 1);
        }
        Ok(())
    }
}
