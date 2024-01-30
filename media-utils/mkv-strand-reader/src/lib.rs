// Copyright 2022-2023 Andrew D. Straw.
use mkv_parser_kit::{ebml_parse, BoxData, EbmlElement, Tag};

const STRAND_MKV_FILENAME_TEMPLATE: &str = "movie%Y%m%d_%H%M%S.%f";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("parse error: {source}")]
    Parser {
        #[from]
        source: mkv_parser_kit::Error,
    },
    #[error("missing data: {missing}")]
    MissingData { missing: String },
    #[error("no cluster timestamp")]
    NoClusterTimestamp,
    #[error("could not determine filename")]
    CouldNotDetermineFilename,
    #[error("filename is not valid UTF-8")]
    FilenameNotUtf8,
    #[error("filename is could not be parsed to datetime")]
    DatetimeInFilenameNotParsedCorrectly,
}

fn missing(what: &str) -> Error {
    Error::MissingData {
        missing: what.to_string(),
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BlockData {
    pub pts: std::time::Duration,
    pub start_idx: u64,
    pub size: usize,
    pub is_keyframe: bool,
}

#[derive(Debug)]
pub struct StrandCamMkvMetadata {
    pub creation_time: chrono::DateTime<chrono::FixedOffset>,
    pub camera_name: Option<String>,
    pub gamma: Option<f32>,
    pub writing_app: String,
}

#[derive(Debug)]
pub struct ParsedStrandCamMkv {
    pub width: u32,
    pub height: u32,
    pub timestep_nanos: u32,
    pub metadata: StrandCamMkvMetadata,
    pub block_data: Vec<BlockData>,
    pub uncompressed_fourcc: Option<String>,
    pub codec: String,
}

impl TryFrom<Accum> for ParsedStrandCamMkv {
    type Error = Error;
    fn try_from(a: Accum) -> Result<Self> {
        let width = a.width.ok_or(missing("width"))?;
        let height = a.height.ok_or(missing("height"))?;
        let metadata = StrandCamMkvMetadata {
            creation_time: a.creation_time.ok_or(missing("creation time"))?,
            gamma: a.gamma,
            // Strand Cam only ever saved the camera name to the MKV title field.
            camera_name: a.title,
            writing_app: a.writing_app.ok_or(missing("writing app"))?,
        };
        Ok(Self {
            width,
            height,
            timestep_nanos: a.timestep_nanos.ok_or(missing("timestep"))?,
            block_data: a.block_data,
            codec: a.codec.ok_or(missing("codec"))?,
            metadata,
            uncompressed_fourcc: a.uncompressed_fourcc,
        })
    }
}

#[derive(Debug, Default)]
struct Accum {
    n_segments: u8,
    width: Option<u32>,
    height: Option<u32>,
    timestep_nanos: Option<u32>,
    creation_time: Option<chrono::DateTime<chrono::FixedOffset>>,
    title: Option<String>,
    gamma: Option<f32>,
    writing_app: Option<String>,
    block_data: Vec<BlockData>,
    /// The timestamp for the entire cluster.
    // segment_cluster_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pts: Option<std::time::Duration>,
    uncompressed_fourcc: Option<String>,
    codec: Option<String>,
}

#[allow(dead_code)]
fn line_summary(e: &EbmlElement) -> String {
    let name = format!("{:?}", e.tag());
    let position = e.position();
    let full_size = e.full_size();
    let data_size = e.data_size();
    format!("{name} at {position} size {full_size} data size {data_size}")
}

const PRINT_ALL: bool = false;

fn do_parse(
    element: &EbmlElement,
    depth: u8,
    accum: &mut Accum,
    tag_path: &[Tag],
    verbose: bool,
    filename: Option<&str>,
) -> Result<()> {
    let verbose_prefix = if verbose {
        let mut prefix = String::new();
        for idx in 0..depth {
            if idx == 0 {
                prefix.push('|');
            } else {
                prefix.push(' ');
            }
        }
        Some(prefix)
    } else {
        None
    };
    if let Some(prefix) = &verbose_prefix {
        if PRINT_ALL {
            println!("{}+ {}", prefix, line_summary(element));
        }
    }
    for child in element.children().iter() {
        let mut child_tag_path = tag_path.to_vec();
        child_tag_path.push(child.tag());
        do_parse(child, depth + 1, accum, &child_tag_path, verbose, filename)?;
    }

    if let Some(prefix) = &verbose_prefix {
        if let Some(bd) = &element.box_data() {
            if !PRINT_ALL {
                println!("{}+ {}", prefix, line_summary(element));
            }
            println!("{prefix}+           {bd:?}");
        }
    }

    match tag_path {
        [Tag::Segment] => {
            accum.n_segments += 1;
            assert_eq!(accum.n_segments, 1); // no support for > 1 segment
        }
        [Tag::Segment, Tag::Info, Tag::DateUTC] => {
            if let Some(BoxData::DateTime(creation_time_utc)) = element.box_data() {
                let creation_time = infer_timezone(creation_time_utc, filename)?;
                accum.creation_time = Some(creation_time);
            } else {
                panic!("need DateUTC");
            }
        }
        [Tag::Segment, Tag::Info, Tag::TimestampScale] => {
            accum.timestep_nanos = Some(get_uint(element));
        }
        [Tag::Segment, Tag::Info, Tag::WritingApp] => {
            accum.writing_app = Some(get_string(element));
        }
        [Tag::Segment, Tag::Info, Tag::Title] => {
            accum.title = Some(get_string(element));
        }
        [Tag::Segment, Tag::Info, Tag::GammaValue] => {
            accum.gamma = Some(get_float(element));
        }
        [Tag::Segment, Tag::Cluster] => {
            // accum.segment_cluster_timestamp = None;
            accum.pts = None;
        }
        [Tag::Segment, Tag::Cluster, Tag::Timestamp] => {
            // assert!(accum.segment_cluster_timestamp.is_none());
            assert!(accum.pts.is_none());
            // convert to u64 because these numbers can get big.
            let n_timesteps: u64 = get_uint(element).into();
            let timestep_nanos: u64 = accum.timestep_nanos.unwrap().into();
            let pts_total_nanos = n_timesteps * timestep_nanos;

            let pts = std::time::Duration::from_nanos(pts_total_nanos);
            // let timestamp = accum.creation_time.unwrap() + pts;
            // accum.segment_cluster_timestamp = Some(timestamp);
            accum.pts = Some(pts);
        }
        [Tag::Segment, Tag::Cluster, Tag::SimpleBlock] => {
            // if let Some(cluster_timestamp) = accum.segment_cluster_timestamp {
            if let Some(cluster_pts) = accum.pts {
                let x = if let Some(BoxData::SimpleBlockData(block_data)) = &element.box_data() {
                    let n_timesteps: u64 = block_data.timestamp.try_into().unwrap();
                    let timestep_nanos: u64 = accum.timestep_nanos.unwrap().into();
                    let cluster_offset_nanos = n_timesteps * timestep_nanos;
                    let cluster_offset = std::time::Duration::from_nanos(cluster_offset_nanos);
                    let pts = cluster_pts + cluster_offset;
                    BlockData {
                        pts,
                        start_idx: block_data.start,
                        is_keyframe: block_data.is_keyframe,
                        size: block_data.size.try_into().unwrap(),
                    }
                } else {
                    panic!("expected UncompressedFourCC in {:?}", element.tag());
                };
                accum.block_data.push(x);
            } else {
                return Err(Error::NoClusterTimestamp);
            }
        }
        [Tag::Segment, Tag::Tracks, Tag::TrackEntry, Tag::CodecID] => {
            accum.codec = Some(get_ascii_string(element));
        }
        [Tag::Segment, Tag::Tracks, Tag::TrackEntry, Tag::Video, Tag::UncompressedFourCC] => {
            accum.uncompressed_fourcc =
                if let Some(BoxData::UncompressedFourCC(s)) = &element.box_data() {
                    Some(s.clone())
                } else {
                    panic!("expected UncompressedFourCC in {:?}", element.tag());
                }
        }
        [Tag::Segment, Tag::Tracks, Tag::TrackEntry, Tag::Video, Tag::PixelWidth] => {
            accum.width = Some(get_uint(element));
        }
        [Tag::Segment, Tag::Tracks, Tag::TrackEntry, Tag::Video, Tag::PixelHeight] => {
            accum.height = Some(get_uint(element));
        }
        _ => {
            // dbg!(&tag_path);
        }
    }
    Ok(())
}

fn get_ascii_string(element: &EbmlElement) -> String {
    if let Some(BoxData::AsciiString(s)) = &element.box_data() {
        s.clone()
    } else {
        panic!("expected ascii string in {:?}", element.tag());
    }
}

fn get_string(element: &EbmlElement) -> String {
    if let Some(BoxData::String(s)) = &element.box_data() {
        s.clone()
    } else {
        panic!("expected string in {:?}", element.tag());
    }
}

fn get_float(element: &EbmlElement) -> f32 {
    if let Some(BoxData::Float(f)) = &element.box_data() {
        *f
    } else {
        panic!("expected string");
    }
}

fn get_uint(element: &EbmlElement) -> u32 {
    if let Some(BoxData::UnsignedInt(v)) = &element.box_data() {
        *v
    } else {
        panic!("expected string");
    }
}

pub fn parse_strand_cam_mkv<R, P>(
    rdr: R,
    verbose: bool,
    path: Option<P>,
) -> Result<(ParsedStrandCamMkv, R)>
where
    R: std::io::Read + std::io::Seek,
    P: AsRef<std::path::Path>,
{
    let filename = path.map(|p| format!("{}", p.as_ref().display()));
    let (parsed, rdr) = ebml_parse(rdr)?;
    // println!("parsing done ---------- ");
    let mut accum = Accum::default();
    for element in parsed.iter() {
        do_parse(
            element,
            0,
            &mut accum,
            &[element.tag()],
            verbose,
            filename.as_deref(),
        )?;
    }
    Ok((accum.try_into()?, rdr))
}

/// Attempt to parse a filename to set the correct timezone.
///
/// Strand Camera (and its predecessors) have saved times in UTC but set
/// filenames based on the local time. This allows us to infer the timezone
/// offset from UTC at the time the file was recorded without losing the UTC
/// time. Here attempt to do this.
pub fn infer_timezone(
    creation_time_utc: &chrono::DateTime<chrono::Utc>,
    filename: Option<&str>,
) -> Result<chrono::DateTime<chrono::FixedOffset>> {
    let zero_offset = chrono::FixedOffset::east_opt(0).unwrap();
    let mut creation_time = creation_time_utc.with_timezone(&zero_offset);
    if let Some(filename) = filename {
        let path_buf = std::path::PathBuf::from(filename);
        let filename = path_buf
            .file_name()
            .ok_or(Error::CouldNotDetermineFilename)?;
        let filename = filename.to_str().ok_or(Error::FilenameNotUtf8)?;

        // Optimisitcally parse filename according to default for
        // `StrandCamArgs::mkv_filename_template`.
        let underscores: Vec<_> = filename.split('_').collect();
        if underscores.len() > 2 {
            let joined = format!("{}_{}", underscores[0], underscores[1]);

            match chrono::NaiveDateTime::parse_from_str(&joined, STRAND_MKV_FILENAME_TEMPLATE) {
                Ok(naive) => {
                    let offset_dur = naive - creation_time_utc.naive_utc();
                    let offset =
                        chrono::FixedOffset::east_opt(offset_dur.num_seconds().try_into().unwrap())
                            .unwrap();
                    creation_time = creation_time_utc.with_timezone(&offset);
                    let test_str = creation_time
                        .format(STRAND_MKV_FILENAME_TEMPLATE)
                        .to_string();
                    // There was a datetime, but we could not parse it
                    // correctly. It's not clear to me if this could ever
                    // happen.
                    if joined != test_str {
                        return Err(Error::DatetimeInFilenameNotParsedCorrectly);
                    }
                }
                Err(_e) => {}
            }
        }
    };
    Ok(creation_time)
}
