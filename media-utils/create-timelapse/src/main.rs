//! create timelapse video from mp4 h264 source without transcoding

use std::io::Read;

use clap::Parser;
use eyre::{Context, Result};
use h264_reader::nal::Nal;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(long)]
    /// Input file
    input: std::path::PathBuf,

    #[arg(long)]
    /// Output file
    output: std::path::PathBuf,

    #[arg(long)]
    /// Interval. This specifies the number of frames between frames used in the
    /// timelapse. After `interval` frames have passed, the next I frame is
    /// inserted into the output.
    interval: usize,

    #[arg(long, default_value = "25.0")]
    /// Frames per second of output video
    fps: f64,

    #[arg(long)]
    /// Set to show progress indicator
    show_progress: bool,
}

struct EbspNal(Vec<u8>);

#[derive(Default)]
struct MyPreParser<'a> {
    n_saved: usize,
    out_path: std::path::PathBuf,
    dt: f64,
    interval: usize,
    count: usize,
    sps: Option<EbspNal>,
    pixel_dimensions: Option<(u32, u32)>,
    pps: Option<EbspNal>,
    cur_seis: Vec<EbspNal>,
    pb: Option<indicatif::ProgressBar>,
    do_save_next: bool,
    mp4_cfg: Option<strand_cam_remote_control::Mp4RecordingConfig>,
    wtr: Option<mp4_writer::Mp4Writer<'a, std::fs::File>>,
}

impl<'a> frame_source::h264_source::H264Preparser for MyPreParser<'a> {
    fn put_seq_param_set(&mut self, nal: &h264_reader::nal::RefNal<'_>) -> Result<()> {
        let isps = h264_reader::nal::sps::SeqParameterSet::from_bits(nal.rbsp_bits()).unwrap();
        self.pixel_dimensions = Some(isps.pixel_dimensions().unwrap());
        let mut ebsp_buf = vec![];
        nal.reader().read_to_end(&mut ebsp_buf)?;
        self.sps = Some(EbspNal(ebsp_buf));
        Ok(())
    }
    fn put_pic_param_set(&mut self, nal: &h264_reader::nal::RefNal<'_>) -> Result<()> {
        let mut ebsp_buf = vec![];
        nal.reader().read_to_end(&mut ebsp_buf)?;
        self.pps = Some(EbspNal(ebsp_buf));
        Ok(())
    }
    fn put_sei_nalu(&mut self, nalu: &h264_reader::nal::RefNal<'_>) -> Result<()> {
        let mut buf = EbspNal(vec![]);
        nalu.reader().read_to_end(&mut buf.0)?;
        self.cur_seis.push(buf);
        Ok(())
    }
    fn put_slice_layer_nalu(
        &mut self,
        nalu: &h264_reader::nal::RefNal<'_>,
        is_i_frame: bool,
    ) -> Result<()> {
        if self.count.is_multiple_of(self.interval) {
            self.do_save_next = true;
        }
        if is_i_frame && self.do_save_next {
            if self.wtr.is_none() {
                let out_fd = std::fs::File::create_new(&self.out_path).with_context(|| {
                    format!(
                        "Creating timelapse output mp4 video: {}",
                        self.out_path.display()
                    )
                })?;
                let mut my_mp4_writer =
                    mp4_writer::Mp4Writer::new(out_fd, self.mp4_cfg.clone().unwrap(), None)?;
                my_mp4_writer.set_first_sps_pps(
                    self.sps.as_ref().map(|x| x.0.clone()),
                    self.pps.as_ref().map(|x| x.0.clone()),
                );
                self.wtr = Some(my_mp4_writer);
            }

            if let Some(wtr) = self.wtr.as_mut() {
                let mut image = EbspNal(vec![]);
                nalu.reader().read_to_end(&mut image.0)?;

                let mut bufs = vec![];

                for buf in self.cur_seis.iter() {
                    bufs.push(buf.0.clone());
                }

                bufs.push(image.0);
                let (w, h) = self.pixel_dimensions.as_ref().unwrap();
                // timestamps only used to calculate PTS, not absolute time
                let frame0_time = chrono::DateTime::from_timestamp(0, 0).unwrap();
                let pts = chrono::TimeDelta::from_std(std::time::Duration::from_secs_f64(
                    self.dt * self.n_saved as f64,
                ))?;
                let timestamp = frame0_time + pts;
                wtr.write_h264_buf(
                    &frame_source::H264EncodingVariant::RawEbsp(bufs),
                    *w,
                    *h,
                    timestamp,
                    frame0_time,
                    false,
                )?;
                self.n_saved += 1;
            }

            self.do_save_next = false;
        }
        self.count += 1;
        self.cur_seis.clear();
        Ok(())
    }
    fn set_num_positions(&mut self, num_positions: usize) -> Result<()> {
        let style = indicatif::ProgressStyle::with_template(
            "Saving timelapse {wide_bar} {pos}/{len} ETA: {eta} ",
        )?;
        self.pb = Some(indicatif::ProgressBar::new(num_positions.try_into()?).with_style(style));
        Ok(())
    }
    fn set_position(&mut self, pos: usize) -> Result<()> {
        if let Some(pb) = self.pb.as_mut() {
            pb.set_position(pos.try_into()?);
        }
        Ok(())
    }
    fn close(self) -> Result<()> {
        if let Some(pb) = self.pb {
            pb.finish_and_clear();
        }
        if let Some(mut wtr) = self.wtr {
            wtr.finish()?;
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    env_tracing_logger::init();
    let cli = Cli::parse();

    if cli.output.extension().unwrap() != "mp4" {
        eyre::bail!("can only save to mp4");
    }

    let cfg = strand_cam_remote_control::Mp4RecordingConfig {
        codec: strand_cam_remote_control::Mp4Codec::H264RawStream,
        max_framerate: Default::default(),
        h264_metadata: None,
    };

    let fps = cli.fps;
    let dt = 1.0 / fps;
    let preparser = MyPreParser {
        dt,
        out_path: cli.output,
        interval: cli.interval,
        mp4_cfg: Some(cfg),
        ..Default::default()
    };
    let _src = frame_source::FrameSourceBuilder::new(&cli.input)
        .show_progress(cli.show_progress)
        .build_h264_in_mp4_source_with_preparser(Box::new(preparser))?;
    Ok(())
}
