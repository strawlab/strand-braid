use mkv_parser_kit::{ebml_parse, BoxData, EbmlElement, Tag};

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MyBlockData {
    timestamp: chrono::DateTime<chrono::Utc>,
    start_idx: u64,
    size: usize,
}

#[derive(Debug, Default)]
struct Accum {
    n_segments: u8,
    width: Option<u32>,
    height: Option<u32>,
    timestep_nanos: Option<u32>,
    creation_time: Option<chrono::DateTime<chrono::Utc>>,
    title: Option<String>,
    gamma: Option<f32>,
    writing_app: Option<String>,
    block_data: Vec<MyBlockData>,
    /// The timestamp for the entire cluster.
    segment_cluster_timestamp: Option<chrono::DateTime<chrono::Utc>>,
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

fn accumulate(element: &EbmlElement, depth: u8, accum: &mut Accum, tag_path: &[Tag]) {
    let mut prefix = String::new();
    for idx in 0..depth {
        if idx == 0 {
            prefix.push('|');
        } else {
            prefix.push(' ');
        }
    }
    if PRINT_ALL {
        println!("{}+ {}", prefix, line_summary(element));
    }
    for child in element.children().iter() {
        let mut child_tag_path = tag_path.to_vec();
        child_tag_path.push(child.tag());
        accumulate(child, depth + 1, accum, &child_tag_path);
    }

    if let Some(bd) = &element.box_data() {
        if !PRINT_ALL {
            println!("{}+ {}", prefix, line_summary(element));
        }
        println!("{prefix}+           {bd:?}");
    }

    match tag_path {
        &[Tag::Segment] => {
            accum.n_segments += 1;
            assert_eq!(accum.n_segments, 1); // no support for > 1 segment
        }
        &[Tag::Segment, Tag::Info, Tag::DateUTC] => {
            if let Some(BoxData::DateTime(creation_time)) = element.box_data() {
                accum.creation_time = Some(*creation_time);
            } else {
                panic!("need DateUTC");
            }
        }
        &[Tag::Segment, Tag::Info, Tag::TimestampScale] => {
            accum.timestep_nanos = Some(get_uint(element));
        }
        &[Tag::Segment, Tag::Info, Tag::WritingApp] => {
            accum.writing_app = Some(get_string(element));
        }
        &[Tag::Segment, Tag::Info, Tag::Title] => {
            accum.title = Some(get_string(element));
        }
        &[Tag::Segment, Tag::Info, Tag::GammaValue] => {
            accum.gamma = Some(get_float(element));
        }
        &[Tag::Segment, Tag::Cluster] => {
            accum.segment_cluster_timestamp = None;
        }
        &[Tag::Segment, Tag::Cluster, Tag::Timestamp] => {
            assert!(accum.segment_cluster_timestamp.is_none());
            // convert to u64 because these numbers can get big.
            let n_timesteps: u64 = get_uint(element).try_into().unwrap();
            let timestep_nanos: u64 = accum.timestep_nanos.unwrap().try_into().unwrap();
            let pts_total_nanos = n_timesteps * timestep_nanos;

            let pts = chrono::Duration::nanoseconds(pts_total_nanos.try_into().unwrap());
            let timestamp = accum.creation_time.unwrap() + pts;
            accum.segment_cluster_timestamp = Some(timestamp);
        }
        &[Tag::Segment, Tag::Cluster, Tag::SimpleBlock] => {
            if let Some(cluster_timestamp) = accum.segment_cluster_timestamp {
                let x = if let Some(BoxData::SimpleBlockData(block_data)) = &element.box_data() {
                    let n_timesteps: u64 = block_data.timestamp.try_into().unwrap();
                    let timestep_nanos: u64 = accum.timestep_nanos.unwrap().try_into().unwrap();
                    let cluster_offset_nanos = n_timesteps * timestep_nanos;
                    let cluster_offset =
                        chrono::Duration::nanoseconds(cluster_offset_nanos.try_into().unwrap());
                    let timestamp = cluster_timestamp + cluster_offset;
                    MyBlockData {
                        timestamp,
                        start_idx: block_data.start,
                        size: block_data.size.try_into().unwrap(),
                    }
                } else {
                    panic!("expected UncompressedFourCC in {:?}", element.tag());
                };
                accum.block_data.push(x);
            } else {
                println!("ERROR: no timestamp as expected.");
            }
        }
        &[Tag::Segment, Tag::Tracks, Tag::TrackEntry, Tag::CodecID] => {
            accum.codec = Some(get_ascii_string(element));
        }
        &[Tag::Segment, Tag::Tracks, Tag::TrackEntry, Tag::Video, Tag::UncompressedFourCC] => {
            accum.uncompressed_fourcc =
                if let Some(BoxData::UncompressedFourCC(s)) = &element.box_data() {
                    Some(s.clone())
                } else {
                    panic!("expected UncompressedFourCC in {:?}", element.tag());
                }
        }
        &[Tag::Segment, Tag::Tracks, Tag::TrackEntry, Tag::Video, Tag::PixelWidth] => {
            accum.width = Some(get_uint(element));
        }
        &[Tag::Segment, Tag::Tracks, Tag::TrackEntry, Tag::Video, Tag::PixelHeight] => {
            accum.height = Some(get_uint(element));
        }
        _ => {
            // dbg!(&tag_path);
        }
    }
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

fn main() -> anyhow::Result<()> {
    let fname = std::env::args().nth(1).unwrap();
    let mut fd = std::fs::File::open(&fname)?;
    let (parsed, _rdr) = ebml_parse(&mut fd).unwrap();
    println!("parsing done ---------- ");
    let mut accum = Accum::default();
    for element in parsed.iter() {
        accumulate(element, 0, &mut accum, &[element.tag()])
    }

    if accum.block_data.is_empty() {
        anyhow::bail!("block data is empty");
    }
    let block_data_0 = accum.block_data[0].clone();
    let n_blocks = accum.block_data.len();
    let block_data_last = accum.block_data[n_blocks - 1].clone();

    let all_block_data = std::mem::take(&mut accum.block_data);
    println!("{accum:?}");

    println!("{block_data_0:?}");
    println!(".. {n_blocks} blocks total ..");
    println!("{block_data_last:?}");
    let dur = block_data_last
        .timestamp
        .signed_duration_since(block_data_0.timestamp);
    let dur_secs = dur.to_std().unwrap().as_secs_f64();
    let fps = (n_blocks - 1) as f64 / dur_secs;
    println!("{dur_secs:.1} secs, {fps:.2} fps");

    for (block_count, block_data) in all_block_data.iter().enumerate() {
        use std::io::{Read, Seek};
        fd.seek(std::io::SeekFrom::Start(block_data.start_idx))?;
        let mut buf = vec![0u8; block_data.size];
        fd.read_exact(&mut buf[..])?;

        println!(
            "block {block_count} at position {} (size {}) time {}",
            block_data.start_idx, block_data.size, block_data.timestamp
        );
        for (line_count, bytes) in buf.chunks(20).enumerate() {
            for byte in bytes.iter() {
                print!("{byte:02X} ");
            }
            println!();
            if line_count > 10 {
                break;
            }
        }
        if block_count > 10 {
            break;
        }
    }
    Ok(())
}
