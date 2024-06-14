// Copyright 2024 Andrew D. Straw.
use bytes::Buf;
use std::io::Read;

use crate::{h264_source::AnnexBLocation, Result};

pub(crate) fn find_nals<R: Read>(mut rdr: R) -> Result<Vec<AnnexBLocation>> {
    let mut read_buf: Vec<u8> = vec![0u8; 4 * 1024 * 1024]; // 4 MB
    let mut buf = bytes::BytesMut::with_capacity(0);
    let mut start_stop: Vec<(usize, usize)> = Vec::new();
    let mut buf_position_in_source: usize = 0;
    let mut cur_nal_start: Option<usize> = None;
    let finder = memchr::memmem::Finder::new(&[0x00, 0x00, 0x01]);
    loop {
        // The outer loop reads and fills the buffer.
        let bufsz = rdr.read(&mut read_buf)?;
        if bufsz == 0 {
            // EOF
            break;
        }
        buf.extend_from_slice(&read_buf[..bufsz]);

        loop {
            // The inner loop looks for the start code.
            let idx = match finder.find(&buf) {
                Some(idx) => idx,
                None => {
                    // No start code, so fill buffer further (or end).
                    break;
                }
            };

            if let Some(prev_nal_start) = cur_nal_start.take() {
                let prev_nal_end = if idx >= 1 && buf[idx - 1] == 0x00 {
                    // 4 byte start code
                    buf_position_in_source + idx - 1
                } else {
                    // 3 byte start code
                    buf_position_in_source + idx
                };
                start_stop.push((prev_nal_start, prev_nal_end));
            }
            cur_nal_start = Some(buf_position_in_source + idx + 3);

            let advance = idx + 3;
            buf.advance(advance);
            buf_position_in_source += advance;
        }
    }
    if let Some(prev_nal_start) = cur_nal_start.take() {
        let prev_nal_end = prev_nal_start + buf.len();
        start_stop.push((prev_nal_start, prev_nal_end));
    }
    Ok(start_stop
        .iter()
        .map(|(start, stop)| AnnexBLocation {
            start: *start as u64,
            sz: stop - start,
        })
        .collect())
}

#[cfg(test)]
mod test {
    use std::collections::VecDeque;

    use super::*;

    /// Implements [std::io::Read] by splitting original buffer.
    struct SplitReader {
        bufs: VecDeque<Vec<u8>>,
    }

    impl SplitReader {
        fn new(buf: &[u8], split: usize) -> Self {
            Self {
                bufs: vec![buf[..split].to_vec(), buf[split..].to_vec()].into(),
            }
        }
    }

    impl std::io::Read for SplitReader {
        fn read(&mut self, outbuf: &mut [u8]) -> std::io::Result<usize> {
            if let Some(inbuf) = self.bufs.pop_front() {
                let sz = inbuf.len();
                if outbuf.len() >= sz {
                    // output is large enough
                    (&mut outbuf[..sz]).copy_from_slice(&inbuf);
                    Ok(sz)
                } else {
                    // need to break up input
                    let sz = outbuf.len();
                    let sendbuf = inbuf[..sz].to_vec();
                    let keepbuf = inbuf[sz..].to_vec();
                    (&mut outbuf[..sz]).copy_from_slice(&sendbuf);
                    self.bufs.push_front(keepbuf);
                    Ok(sz)
                }
            } else {
                Ok(0)
            }
        }
    }

    #[test]
    fn test_3byte_synthetic() -> Result<()> {
        // 3 byte variant
        let buf = &[0, 0, 1, 9, 0, 10, 0, 0, 1, 3, 20, 0, 0, 1, 99, 99];
        for split in 1..buf.len() {
            let rdr = SplitReader::new(buf, split);
            let results = find_nals(rdr)?;

            assert_eq!(results.len(), 3);
            assert_eq!(results[0], AnnexBLocation { start: 3, sz: 3 });
            assert_eq!(results[1], AnnexBLocation { start: 9, sz: 2 });
            assert_eq!(results[2], AnnexBLocation { start: 14, sz: 2 });
        }
        Ok(())
    }

    #[test]
    fn test_4byte_synthetic() -> Result<()> {
        // 4 byte variant
        let buf = &[0, 0, 0, 1, 9, 0, 10, 0, 0, 0, 1, 3, 20, 0, 0, 0, 1, 99, 99];
        for split in 1..buf.len() {
            let rdr = SplitReader::new(buf, split);
            let results = find_nals(rdr)?;
            assert_eq!(results.len(), 3);
            assert_eq!(results[0], AnnexBLocation { start: 4, sz: 3 });
            assert_eq!(results[1], AnnexBLocation { start: 11, sz: 2 });
            assert_eq!(results[2], AnnexBLocation { start: 17, sz: 2 });
        }
        Ok(())
    }

    #[test]
    fn test_real_file() -> Result<()> {
        let buf = include_bytes!("test-data/test_less-avc_mono8_15x14.h264");
        for split in 1..buf.len() {
            let rdr = SplitReader::new(buf, split);
            let results = find_nals(rdr)?;
            assert_eq!(results.len(), 3);
            assert_eq!(results[0], AnnexBLocation { start: 4, sz: 9 });
            assert_eq!(results[1], AnnexBLocation { start: 17, sz: 4 });
            assert_eq!(results[2], AnnexBLocation { start: 25, sz: 278 });
        }
        Ok(())
    }
}
