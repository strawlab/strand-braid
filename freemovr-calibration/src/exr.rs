/*

Modified from commit 274b898629cb1ddee79f6adbb7076c98ef384d47
of https://github.com/sdao/gammaray. The code has the following license:

gammaray License
================
Copyright (c) 2015-2017, Steven Dao.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

*/

use crate::FloatImage;
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};

const MAGIC_NUMBER: i32 = 20000630;
const VERSION: i32 = 2;
const PIXEL_TYPE_FLOAT: i32 = 2;
const COMPRESSION_NONE: u8 = 0;
const LINE_ORDER_INCREASING_Y: u8 = 0;

fn index(row: usize, col: usize, width: usize) -> usize {
    row * width + col
}

pub struct ExrWriter {
    buffer: std::vec::Vec<u8>,
    width: usize,
    height: usize,
    data_offset: usize,
}

impl ExrWriter {
    pub fn new() -> ExrWriter {
        ExrWriter {
            buffer: vec![],
            width: 0,
            height: 0,
            data_offset: 0,
        }
    }

    fn write_header(&mut self) {
        self.buffer.write_i32::<LittleEndian>(MAGIC_NUMBER).unwrap();
        self.buffer.write_i32::<LittleEndian>(VERSION).unwrap();
    }

    fn write_str(&mut self, s: &str) {
        self.buffer.extend_from_slice(s.as_bytes());
        self.buffer.push(0);
    }

    fn write_channels_attr(&mut self) {
        self.write_str("channels");
        self.write_str("chlist");

        let size: i32 = 2 * 3 +  // Three channels named B, G, R, plus a null-terminator for each.
                16 * 3 + // Four ints (16 bytes) of data per channel.
                1; // One extra null byte.
        self.buffer.write_i32::<LittleEndian>(size).unwrap();

        for channel in ["B", "G", "R"].iter() {
            self.write_str(channel);
            self.buffer
                .write_i32::<LittleEndian>(PIXEL_TYPE_FLOAT)
                .unwrap();
            self.buffer.write_i32::<LittleEndian>(0).unwrap(); // pLinear and reserved
            self.buffer.write_i32::<LittleEndian>(1).unwrap(); // xSampling
            self.buffer.write_i32::<LittleEndian>(1).unwrap(); // ySampling
        }
        self.buffer.push(0); // Null terminator.
    }

    fn write_compression_attr(&mut self) {
        self.write_str("compression");
        self.write_str("compression");
        self.buffer.write_i32::<LittleEndian>(1).unwrap(); // Size = 1 byte.
        self.buffer.push(COMPRESSION_NONE);
    }

    fn write_data_display_window_attrs(&mut self, width: usize, height: usize) {
        let size = 4 * 4; // 4 ints = 16 bytes.
        let window = [0, 0, width as i32 - 1, height as i32 - 1];

        self.write_str("dataWindow");
        self.write_str("box2i");
        self.buffer.write_i32::<LittleEndian>(size).unwrap();
        for i in window.iter() {
            self.buffer.write_i32::<LittleEndian>(*i).unwrap();
        }

        self.write_str("displayWindow");
        self.write_str("box2i");
        self.buffer.write_i32::<LittleEndian>(size).unwrap();
        for i in window.iter() {
            self.buffer.write_i32::<LittleEndian>(*i).unwrap();
        }
    }

    fn write_line_order_attr(&mut self) {
        self.write_str("lineOrder");
        self.write_str("lineOrder");
        self.buffer.write_i32::<LittleEndian>(1).unwrap(); // Size = 1 byte.
        self.buffer.push(LINE_ORDER_INCREASING_Y);
    }

    fn write_pixel_aspect_ratio_attr(&mut self) {
        self.write_str("pixelAspectRatio");
        self.write_str("float");
        self.buffer.write_i32::<LittleEndian>(4).unwrap(); // 1 float = 4 bytes.
        self.buffer.write_f32::<LittleEndian>(1.0).unwrap();
    }

    fn write_screen_window_center_attr(&mut self) {
        self.write_str("screenWindowCenter");
        self.write_str("v2f");
        self.buffer.write_i32::<LittleEndian>(8).unwrap(); // 2 floats = 8 bytes.
        self.buffer.write_f32::<LittleEndian>(0.0).unwrap();
        self.buffer.write_f32::<LittleEndian>(0.0).unwrap();
    }

    fn write_screen_window_width(&mut self, width: usize) {
        self.write_str("screenWindowWidth");
        self.write_str("float");
        self.buffer.write_i32::<LittleEndian>(4).unwrap(); // 1 float = 4 bytes.
        self.buffer.write_f32::<LittleEndian>(width as f32).unwrap();
    }

    fn write_comments(&mut self, comment: &str) {
        self.write_str("comments");
        self.write_str("string");
        self.buffer
            .write_i32::<LittleEndian>(comment.as_bytes().len() as i32)
            .unwrap();
        self.buffer.extend_from_slice(comment.as_bytes());
    }

    fn write_line_offset_table(&mut self, film: &FloatImage) {
        let table_size = 8 * film.height; // 1 ulong (8 bytes) per line.
        let data_offset = self.buffer.len() + table_size;

        // Scan line number (int); bytes in line (uint); RGB (3 floats * 4 bytes) per pixel.
        let line_size = 4 + 4 + (film.width * 4 * 3);

        for y in 0..film.height {
            let line_offset = data_offset + y * line_size;
            self.buffer
                .write_u64::<LittleEndian>(line_offset as u64)
                .unwrap();
        }

        debug_assert!(self.buffer.len() == data_offset);
    }

    fn write_channels(&mut self, film: &FloatImage) {
        // Scan line number (int); bytes in line (uint); RGB (3 floats * 4 bytes) per pixel.
        let line_size = 4 + 4 + (film.width * 4 * 3);
        let data_size = film.height * line_size;

        self.buffer.resize(self.data_offset + data_size, 0);
        let data = &mut self.buffer[self.data_offset..(self.data_offset + data_size)];

        data.chunks_mut(line_size)
            .enumerate()
            .for_each(|(y, line)| {
                LittleEndian::write_i32(&mut line[0..4], y as i32); // Scan line number.
                LittleEndian::write_u32(&mut line[4..8], line_size as u32 - 8); // Bytes in line.

                // let first_pixel = index(film.height - y - 1, 0, film.width);
                let first_pixel = index(y, 0, film.width);
                for i in 0..film.width {
                    let pixel = &film.pixels[first_pixel + i];
                    let val = [pixel.0 as f32, pixel.1 as f32, pixel.2 as f32];
                    let z = 8 + (0 * film.width + i) * 4;
                    let y = 8 + (1 * film.width + i) * 4;
                    let x = 8 + (2 * film.width + i) * 4;
                    LittleEndian::write_f32(&mut line[z..(z + 4)], val[2]);
                    LittleEndian::write_f32(&mut line[y..(y + 4)], val[1]);
                    LittleEndian::write_f32(&mut line[x..(x + 4)], val[0]);
                }
            });
    }

    pub fn update(&mut self, film: &FloatImage, comment: Option<&str>) {
        if self.width != film.width || self.height != film.height {
            // Re-initializate the buffer with the EXR file layout.
            self.buffer.clear();
            self.width = film.width;
            self.height = film.height;

            // Begin header.
            self.write_header();
            self.write_channels_attr();
            self.write_compression_attr();
            self.write_data_display_window_attrs(film.width, film.height);
            self.write_line_order_attr();
            self.write_pixel_aspect_ratio_attr();
            self.write_screen_window_center_attr();
            self.write_screen_window_width(film.width);
            if let Some(comment) = comment {
                self.write_comments(comment);
            }
            self.buffer.push(0); // End header.

            // Begin line offset table.
            self.write_line_offset_table(film); // End line offset table.
            self.data_offset = self.buffer.len();
        }

        // Begin data. This will resize the buffer the first time around, but will overwrite the
        // buffer on subsequent rounds.
        self.write_channels(film); // End data.
    }

    pub fn buffer(self) -> Vec<u8> {
        self.buffer
    }
}
