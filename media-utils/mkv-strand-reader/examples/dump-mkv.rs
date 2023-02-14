// Copyright 2022-2023 Andrew D. Straw.
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Sets input file name
    input_fname: std::path::PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let fname = cli.input_fname;
    let mut fd = std::fs::File::open(&fname)?;

    let (mut parsed, _rdr) = mkv_strand_reader::parse_strand_cam_mkv(&mut fd, false, Some(&fname))?;

    let block_data_0 = parsed.block_data[0].clone();
    let n_blocks = parsed.block_data.len();
    let block_data_last = parsed.block_data[n_blocks - 1].clone();

    let all_block_data = std::mem::take(&mut parsed.block_data);
    println!("{parsed:?}");

    println!("{block_data_0:?}");
    println!(".. {n_blocks} blocks total ..");
    println!("{block_data_last:?}");
    let dur = block_data_last.pts;
    let dur_secs = dur.as_secs_f64();
    let fps = (n_blocks - 1) as f64 / dur_secs;
    println!("{dur_secs:.1} secs, {fps:.2} fps");

    let mut prev_pts_f64 = None;

    for (block_count, block_data) in all_block_data.iter().enumerate() {
        use std::io::{Read, Seek};
        fd.seek(std::io::SeekFrom::Start(block_data.start_idx))?;
        let mut buf = vec![0u8; block_data.size];
        fd.read_exact(&mut buf[..])?;

        let pts_f64 = block_data.pts.as_secs_f64();
        let time_delta_msec_fmt = if let Some(prev_pts) = prev_pts_f64 {
            let delta = pts_f64 - prev_pts;
            format!(" (delta {}ms)", delta * 1000.0)
        } else {
            "".to_string()
        };
        prev_pts_f64 = Some(pts_f64);

        println!(
            "block {block_count} at position {} (size {}) time {}ms{}",
            block_data.start_idx,
            block_data.size,
            pts_f64 * 1000.0,
            time_delta_msec_fmt
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
