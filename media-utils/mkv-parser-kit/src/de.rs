// Copyright 2022-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use std::io::{Read, Seek};

use super::ebml_types::{EbmlElement, Tag};
use super::error::{Error, Result};

/// Decodes pickle streams into values.
pub struct Deserializer<'a, R: Read + Seek> {
    rdr: &'a mut R,
    /// cached next element
    element: Option<EbmlElement>,
    /// current position from start of file
    position: u64,
    depth: u8,
}

impl<'a, R: Read + Seek> Deserializer<'a, R> {
    pub fn from_reader(rdr: &'a mut R, position: u64, depth: u8) -> Self {
        Deserializer {
            rdr,
            element: None,
            position,
            depth,
        }
    }

    fn fill_next_element(&mut self) -> Result<()> {
        let position = self.position;
        // println!(
        //     "[{}:{}] depth {} reading at position {}",
        //     file!(),
        //     line!(),
        //     self.depth,
        //     position
        // );
        let (id, data_size, header_size) = match super::parser::read_element_id_size(&mut self.rdr)
        {
            Ok(v) => v,
            Err(Error::Io(e)) => {
                if std::io::ErrorKind::UnexpectedEof == e.kind() {
                    return Err(Error::Eof);
                } else {
                    return Err(Error::Io(e));
                }
            }
            Err(e) => {
                return Err(e);
            }
        };
        // println!(
        //     "[{}:{}] depth {} done reading header (size {header_size}, data size {data_size}) at {position}",file!(),line!(),
        //     self.depth
        // );
        assert!(data_size + header_size > 0);
        self.position += header_size;
        let full_size = data_size + header_size;
        let tag = Tag::from(id);
        let mut children = Vec::new();
        // let mut frame_data = None;
        let mut box_data = None;

        match tag.dtype() {
            b'm' => {
                // Get children from master data type.
                let mut inner_deser =
                    crate::de::Deserializer::from_reader(self.rdr, self.position, self.depth + 1);
                let mut child_size = 0;
                while child_size < data_size {
                    let child = inner_deser.next().ok_or(Error::Eof)??;
                    child_size += child.full_size;
                    children.push(child);
                }
                self.position = inner_deser.position;
            }
            b'b' => {
                match tag {
                    Tag::SimpleBlock => {
                        let mut remaining_data_size = data_size;
                        // see https://web.archive.org/web/20200614123448/https://www.matroska.org/technical/basics.html
                        // and https://matroska.sourceforge.net/technical/specs/index.html#simpleblock_structure
                        type BitReader<R> = bitstream_io::BitReader<R, bitstream_io::BigEndian>;
                        let mut r = BitReader::new(&mut self.rdr);
                        let (track_number, len) = super::parser::read_element_size(&mut r)?;
                        self.position += len;
                        remaining_data_size -= len;

                        let mut timestamp_buf = [0u8; 2];

                        self.rdr
                            .read_exact(&mut timestamp_buf[..])
                            .map_err(Error::Io)?;
                        let timestamp = i16::from_be_bytes(timestamp_buf);
                        self.position += timestamp_buf.len() as u64;
                        remaining_data_size -= timestamp_buf.len() as u64;

                        let mut flags_buf = [0u8];
                        self.rdr.read_exact(&mut flags_buf[..]).map_err(Error::Io)?;
                        let flags = flags_buf[0];
                        self.position += flags_buf.len() as u64;
                        remaining_data_size -= flags_buf.len() as u64;

                        // parse flags --------
                        fn is_bit_set(flags: u8, bit: u8) -> bool {
                            let bit_real_le = 7 - bit;
                            let mask = (0x01 << bit_real_le) as u8;
                            (flags & mask) != 0
                        }

                        // "Bit 0 is the most significant bit."
                        let is_keyframe = is_bit_set(flags, 0);
                        debug_assert!(!is_bit_set(flags, 1)); // reserved zero bits
                        debug_assert!(!is_bit_set(flags, 2)); // reserved zero bits
                        debug_assert!(!is_bit_set(flags, 3)); // reserved zero bits
                        let is_invisible = is_bit_set(flags, 4);
                        let lacing0 = is_bit_set(flags, 5);
                        let lacing1 = is_bit_set(flags, 6);
                        let is_discardable = is_bit_set(flags, 7);
                        // ---------------------

                        if lacing0 || lacing1 {
                            todo!("lacing support");
                        } else {
                            // read frame data -----

                            // let mut frame_buf = vec![0u8; remaining_data_size.try_into().unwrap()];
                            // self.rdr.read_exact(&mut frame_buf).map_err(Error::Io)?;
                            // self.position += remaining_data_size;
                            // remaining_data_size -= remaining_data_size;

                            // remaining data in block: skip
                            self.rdr
                                .seek(std::io::SeekFrom::Current(
                                    remaining_data_size.try_into().unwrap(),
                                ))
                                .map_err(Error::Io)?;
                            box_data = Some(super::ebml_types::BoxData::SimpleBlockData(
                                super::ebml_types::BlockData {
                                    start: self.position,
                                    size: remaining_data_size,
                                    is_keyframe,
                                    is_discardable,
                                    is_invisible,
                                    track_number,
                                    timestamp,
                                },
                            ));
                            self.position += remaining_data_size;
                        }
                    }
                    Tag::UncompressedFourCC => {
                        let mut buf = vec![0u8; data_size.try_into().unwrap()];
                        self.rdr.read_exact(&mut buf).map_err(Error::Io)?;
                        self.position += data_size;

                        let buf: [u8; 4] = buf.try_into().unwrap();
                        box_data = Some(super::ebml_types::BoxData::UncompressedFourCC(
                            String::from_utf8(buf.to_vec()).unwrap(),
                        ));
                    }
                    _ => {
                        // This is b'b' type but not Simpleblock  - we ignore it.

                        // let mut buf = vec![0u8; data_size.try_into().unwrap()];
                        // self.rdr.read_exact(&mut buf).map_err(Error::Io)?;
                        self.rdr
                            .seek(std::io::SeekFrom::Current(data_size.try_into().unwrap()))
                            .map_err(Error::Io)?;
                        self.position += data_size;
                    }
                }
            }
            b'u' | b'f' | b'd' | b'8' | b's' => {
                let mut buf = vec![0u8; data_size.try_into().unwrap()];
                self.rdr.read_exact(&mut buf).map_err(Error::Io)?;
                self.position += data_size;
                use crate::ebml_types::BoxData;
                box_data = Some(match tag.dtype() {
                    b'u' => {
                        assert!(buf.len() <= 4);
                        let val = match buf.len() {
                            1 => buf[0].into(),
                            2 => {
                                let buf: [u8; 2] = buf.try_into().unwrap();
                                u16::from_be_bytes(buf).into()
                            }
                            3 => {
                                let mut buf4 = [0u8; 4];
                                buf4[1..].copy_from_slice(&buf);
                                u32::from_be_bytes(buf4)
                            }
                            4 => {
                                let buf: [u8; 4] = buf.try_into().unwrap();
                                u32::from_be_bytes(buf)
                            }
                            len => {
                                panic!("unsupported unsigned int length {len}");
                            }
                        };
                        BoxData::UnsignedInt(val)
                    }
                    b'f' => {
                        if buf.len() == 4 {
                            let buf: [u8; 4] = buf.try_into().unwrap();
                            let val = f32::from_be_bytes(buf);
                            BoxData::Float(val)
                        } else if buf.len() == 8 {
                            let buf: [u8; 8] = buf.try_into().unwrap();
                            let val = f64::from_be_bytes(buf);
                            BoxData::Float64(val)
                        } else {
                            return Err(Error::BufSizeError(buf.len()));
                        }
                    }
                    b'd' => {
                        let buf: [u8; 8] = buf.try_into().unwrap();
                        let nsecs = u64::from_be_bytes(buf);

                        use chrono::TimeZone;
                        let millennium_exploded =
                            chrono::Utc.with_ymd_and_hms(2001, 1, 1, 0, 0, 0).unwrap();
                        // let elapsed = timestamp.signed_duration_since(millennium_exploded);
                        // let nanoseconds = elapsed.num_nanoseconds().expect("nanosec overflow");
                        let elapsed = chrono::Duration::nanoseconds(nsecs.try_into().unwrap());
                        let datetime = millennium_exploded + elapsed;
                        BoxData::DateTime(datetime)
                    }
                    b'8' => BoxData::String(String::from_utf8(buf).unwrap()),
                    b's' => BoxData::AsciiString(String::from_utf8(buf).unwrap()),
                    _ => {
                        unreachable!();
                    }
                });
            }

            other => {
                if other.is_ascii() {
                    let val = other as char;
                    todo!("handle dtype b'{val}'");
                } else {
                    todo!("handle dtype {other:#X}");
                }
            }
        }

        self.element = Some(EbmlElement {
            tag,
            position,
            full_size,
            data_size,
            children,
            // frame_data,
            box_data,
        });

        Ok(())
    }
}

impl<R: Read + Seek> Iterator for Deserializer<'_, R> {
    type Item = Result<EbmlElement>;
    fn next(&mut self) -> Option<Result<EbmlElement>> {
        if self.element.is_none() {
            match self.fill_next_element() {
                Ok(()) => {}
                Err(Error::Eof) => return None,
                Err(e) => {
                    return Some(Err(e));
                }
            }
        }
        self.element.take().map(Ok)
    }
}
