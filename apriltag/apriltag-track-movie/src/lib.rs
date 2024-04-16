use std::{
    io::Write,
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
};

use anyhow::{Context, Result};
use clap::Parser;
use machine_vision_formats::pixel_format::Mono8;
use serde::{Deserialize, Serialize};

use ads_apriltag as apriltag;
use convert_image::Y4MFrame;

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    /// The input video filename
    pub input_video: std::path::PathBuf,

    /// Maximum number of frames to analyze
    pub max_num_frames: Option<usize>,
}

// The center pixel of the detection is (h02,h12)
#[derive(Serialize, Deserialize, Debug, Clone)]
struct DetectionSerializer {
    frame: usize,
    // time_microseconds: i64,
    id: i32,
    hamming: i32,
    decision_margin: f32,
    h00: f64,
    h01: f64,
    h02: f64,
    h10: f64,
    h11: f64,
    h12: f64,
    h20: f64,
    h21: f64,
    // no h22 because it is always 1.0
    family: String,
}

struct FfmpegFrameIterator {
    y4m_decoder: y4m::Decoder<ChildStdout>,
}

impl FfmpegFrameIterator {
    fn new<P: AsRef<Path>>(fname: P) -> Result<(Self, Child)> {
        #[rustfmt::skip]
        let args = [
            "-nostdin",
            "-i", &format!("{}", fname.as_ref().display()),
            "-f", "yuv4mpegpipe",
            "pipe:",
        ];
        let mut ffmpeg_child = Command::new("ffmpeg")
            .args(args)
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("When spawning: ffmpeg {:?}", args))?;
        let ffmpeg_out = ffmpeg_child.stdout.take().unwrap();

        let y4m_decoder = y4m::decode(ffmpeg_out)?;

        Ok((Self { y4m_decoder }, ffmpeg_child))
    }
}

impl Iterator for FfmpegFrameIterator {
    type Item = Result<Y4MFrame>;
    fn next(&mut self) -> std::option::Option<Self::Item> {
        let w = self.y4m_decoder.get_width().try_into().unwrap();
        let h = self.y4m_decoder.get_height().try_into().unwrap();
        match self.y4m_decoder.read_frame() {
            Ok(f) => Some(Y4MFrame::new_mono8(f.get_y_plane().to_vec(), w, h).map_err(Into::into)),
            Err(y4m::Error::EOF) => None,
            Err(err) => {
                panic!("unexpected error: {err}");
            }
        }
    }
}

fn my_round(a: f32) -> f32 {
    let b = (a * 10.0).round() as i64;
    b as f32 / 10.0
}

fn to_serializer(
    orig: &apriltag::Detection,
    frame: usize,
    // time_microseconds: i64,
) -> DetectionSerializer {
    let h = orig.h();
    // We are not going to save h22, so (in debug builds) let's check it meets
    // our expectations.
    debug_assert!((h[8] - 1.0).abs() < 1e-16);
    DetectionSerializer {
        frame,
        // time_microseconds,
        id: orig.id(),
        hamming: orig.hamming(),
        decision_margin: my_round(orig.decision_margin()),
        h00: h[0],
        h01: h[1],
        h02: h[2],
        h10: h[3],
        h11: h[4],
        h12: h[5],
        h20: h[6],
        h21: h[7],
        family: orig.family_type().to_str().to_string(),
    }
}

pub fn run_cli(cli: Cli) -> Result<()> {
    let mut td = apriltag::Detector::new();
    td.add_family(apriltag::Family::new_tag_standard_41h12());
    td.add_family(apriltag::Family::new_tag_36h11());

    let raw_td = td.as_mut();
    // raw_td.debug = 1;
    raw_td.quad_decimate = 2.0;
    raw_td.quad_sigma = 0.0;
    raw_td.refine_edges = 1;
    raw_td.decode_sharpening = 0.25;

    let csv_output_fname = format!("{}.csv", cli.input_video.display());

    println!("Decoding input: {}", cli.input_video.display());
    println!("Will output to:");
    println!("{}", &csv_output_fname);
    let (mut frames, mut ffmpeg_child) = FfmpegFrameIterator::new(&cli.input_video)?;

    let mut frame_store;

    let frame_iter: &mut dyn Iterator<Item = Result<Y4MFrame, anyhow::Error>> =
        if let Some(max_num_frames) = cli.max_num_frames {
            frame_store = Some(frames.take(max_num_frames));
            frame_store.as_mut().unwrap()
        } else {
            &mut frames
        };

    let mut wtr = None;

    for (frame, y4m_frame) in frame_iter.enumerate() {
        let y4m_frame = y4m_frame?;
        let decoded_mono8 = y4m_frame.convert::<Mono8>()?;
        if false {
            // This block for debugging ffmpeg video decoding.
            let png_buf =
                convert_image::frame_to_image(&decoded_mono8, convert_image::ImageOptions::Png)?;
            let fname = format!("frame{frame:09}.png");
            println!("saving png {fname}");
            let mut file = std::fs::File::create(&fname)?;
            file.write_all(&png_buf)?;
        }

        let im = apriltag::ImageU8Borrowed::view(&decoded_mono8);
        let detections = td.detect(apriltag::ImageU8::inner(&im));

        if !detections.is_empty() {
            if wtr.is_none() {
                let mut fd = std::fs::File::create(&csv_output_fname)?;
                writeln!(
                    fd,
                    "# The homography matrix entries (h00,...) are described in the April Tags paper"
                )?;
                writeln!(
                    fd,
                    "# https://dx.doi.org/10.1109/ICRA.2011.5979561 . Entry h22 is not saved because"
                )?;
                writeln!(
                    fd,
                    "# it always has value 1. The center pixel of the detection is (h02,h12)."
                )?;
                wtr = Some(csv::Writer::from_writer(fd));
            }

            for det in detections.as_slice().iter() {
                let atd: DetectionSerializer = to_serializer(det, frame); //, time_microseconds);
                wtr.as_mut().unwrap().serialize(atd)?;
            }
        }
    }

    ffmpeg_child.kill()?;
    Ok(())
}
