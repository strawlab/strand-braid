use std::collections::VecDeque;

use anyhow::{Context as ContextTrait, Result};
use chrono::{DateTime, Utc};

use ffmpeg::format::Pixel;
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{context::Context, flag::Flags};
use ffmpeg::util::frame::video::Video;
use ffmpeg_next as ffmpeg;

use crate::MovieReader;
use basic_frame::DynamicFrame;

/// Convert a Result<T,E> into Option<Result<T,E>> and return Some(Err(E)) on error.
macro_rules! try_iter {
    ($x:expr) => {
        match $x {
            Ok(val) => val,
            Err(e) => {
                return Some(Err(e.into()));
            }
        }
    };
}

/// Read a file frame-by-frame.
///
/// Since the ffmpeg api reads packet-by-packet, we need something to return
/// frame-by-frame. This must necessarily decode the packets into frames.
pub struct FfmpegFrameReader {
    /// The filename of the file
    pub(crate) filename: String,
    /// Creation time of this particular frame reader
    pub(crate) creation_time: DateTime<Utc>,
    /// The ffmpeg input
    ictx: ffmpeg::format::context::Input,
    /// The ffmpeg decoder
    decoder: ffmpeg::decoder::Video,
    /// The ffmpeg scaler if needed
    scaler: Option<Context>,
    /// Where the video stream starts in the file
    video_stream_index: usize,
    /// Frames already decoded awaiting consumption
    frame_queue: VecDeque<DynamicFrame>,
    /// Have we reached the end of the file?
    file_done: bool,
    time_base: ffmpeg::Rational,
    pub(crate) title: Option<String>,
    count: usize,
    hack_fix_speed: u64,
}

impl FfmpegFrameReader {
    pub fn new(filename: &str) -> Result<Self> {
        let ictx = ffmpeg::format::input(&filename)
            .with_context(|| anyhow::anyhow!("Error from ffmpeg opening '{}'", &filename))?;
        let metadata = ictx.metadata();
        let creation_time_str = metadata.get("creation_time").unwrap();
        let creation_time: DateTime<chrono::FixedOffset> =
            chrono::DateTime::parse_from_rfc3339(creation_time_str)?;
        let creation_time = creation_time.into();
        let title = metadata.get("title").map(Into::into);

        let stream = ictx
            .streams()
            .best(Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;

        let video_stream_index = stream.index();
        let time_base = stream.time_base();
        log::debug!("filename: {}, timebase {:?}", filename, time_base);

        let context_decoder =
            ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context_decoder.decoder().video()?;
        let scaler = match decoder.format() {
            Pixel::GRAY8 | Pixel::RGB24 => None,
            _ => Some(Context::get(
                decoder.format(),
                decoder.width(),
                decoder.height(),
                Pixel::RGB24,
                decoder.width(),
                decoder.height(),
                Flags::BILINEAR,
            )?),
        };

        Ok(Self {
            filename: filename.to_string(),
            creation_time,
            decoder,
            scaler,
            ictx,
            video_stream_index,
            frame_queue: VecDeque::new(),
            file_done: false,
            time_base,
            title,
            count: 0,
            hack_fix_speed: 1,
        })
    }

    pub fn set_hack_fix_speed(&mut self, hack_fix_speed: u64) {
        self.hack_fix_speed = hack_fix_speed;
    }

    /// The decoder has been given new information, so update our frame queue
    /// with this new information.
    ///
    /// Returns true if a new frame is available.
    fn pump_decoder(&mut self) -> Result<bool> {
        let mut frame_available = false;
        let mut video_input = Video::empty();
        let scale =
            (1e9 * self.time_base.numerator() as f64 / self.time_base.denominator() as f64) as u64;
        while self.decoder.receive_frame(&mut video_input).is_ok() {
            let pts = video_input.pts().unwrap();
            let nanosecs = pts as u64 * scale * self.hack_fix_speed;
            log::debug!("pts {}, scale {}, nanosecs {}", pts, scale, nanosecs);
            let pts_chrono = self
                .creation_time
                .checked_add_signed(
                    chrono::Duration::from_std(std::time::Duration::from_nanos(nanosecs)).unwrap(),
                )
                .unwrap();

            let frame_data = {
                // We handle lifetime issues with somewhat complex scopes.

                // Allocate video output if needed.
                let mut alloced_video_output = if self.scaler.is_some() {
                    Some(Video::empty())
                } else {
                    None
                };

                // Now fill the video output using FFMPEG's scaler if needed
                // else just copying the reference to the input if not.
                let video_output = if let Some(scaler) = &mut self.scaler {
                    // let ffmpeg convert data to RGB24
                    {
                        let mut video_output = alloced_video_output.as_mut().unwrap();
                        scaler.run(&video_input, &mut video_output)?;
                    }
                    // We allocated this just above, to this unwrap will never
                    // panic.
                    alloced_video_output.as_ref().unwrap()
                } else {
                    // We support this format directly
                    &video_input
                };

                // convert from ffmpeg to basic_frame::DynamicFrame
                let width = video_output.width();
                let height = video_output.height();
                let stride = video_output.stride(0).try_into().unwrap();

                let ffmpeg_fmt = video_output.format();
                let pixel_format = if ffmpeg_fmt == Pixel::RGB24 {
                    machine_vision_formats::PixFmt::RGB8
                } else {
                    assert_eq!(ffmpeg_fmt, Pixel::GRAY8);
                    machine_vision_formats::PixFmt::Mono8
                };

                let image_data = video_output.data(0).to_vec();
                let extra = Box::new(basic_frame::BasicExtra {
                    host_timestamp: pts_chrono,
                    host_framenumber: self.count,
                });

                basic_frame::DynamicFrame::new(
                    width,
                    height,
                    stride,
                    extra,
                    image_data,
                    pixel_format,
                )
            };
            self.frame_queue.push_back(frame_data);
            frame_available = true;
            self.count += 1;
        }

        Ok(frame_available)
    }
}

impl MovieReader for FfmpegFrameReader {
    fn title(&self) -> Option<&str> {
        self.title.as_ref().map(|x| x.as_str())
    }
    fn filename(&self) -> &str {
        &self.filename
    }
    fn creation_time(&self) -> &DateTime<Utc> {
        &self.creation_time
    }

    /// Get the next frame
    ///
    /// Iterate over packets but return frames
    fn next_frame(&mut self) -> Option<Result<DynamicFrame>> {
        // Do we already have a frame waiting?
        if let Some(frame) = self.frame_queue.pop_front() {
            // If yes, return it.
            return Some(Ok(frame));
        }

        // Have we already finished reading the file?
        if self.file_done {
            // If yes, there is nothing more to do and no new frames will come.
            return None;
        }

        // Read packets and pump the decoder with each new packet. Note that we
        // return from this loop when a new frame is completed and thus
        // subsequent calls to this function may re-start the loop.
        while let Some((stream, packet)) = self.ictx.packets().next() {
            // Get a packet iterator from our current position in the file.
            // Notably, `.packets()` does NOT start from the first packet in the
            // input on subsequent calls, but rather continues from the last
            // location.

            // Handle packet if it is a video packet.
            if stream.index() == self.video_stream_index {
                try_iter!(self.decoder.send_packet(&packet));
                let frame_available = try_iter!(self.pump_decoder());
                if frame_available {
                    return Some(Ok(self.frame_queue.pop_front().unwrap()));
                }
            }
        }
        self.file_done = true;
        try_iter!(self.decoder.send_eof());
        try_iter!(self.pump_decoder());
        self.frame_queue.pop_front().map(Ok)
    }
}
