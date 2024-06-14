// TODO: store slices to memory allocated elsewhere rather than copying.
struct MyNalFragmentHandler {
    nals: Vec<Vec<u8>>,
}

impl h264_reader::push::NalFragmentHandler for MyNalFragmentHandler {
    fn nal_fragment(&mut self, bufs: &[&[u8]], _is_end: bool) {
        for buf in bufs {
            self.nals.push(buf.to_vec());
        }
    }
}

pub(crate) fn h264_annexb_split(large_buf: &[u8]) -> impl Iterator<Item = Vec<u8>> {
    let mut rdr = h264_reader::annexb::AnnexBReader::for_fragment_handler(MyNalFragmentHandler {
        nals: Vec::new(),
    });
    rdr.push(large_buf);
    rdr.into_fragment_handler().nals.into_iter()
}

#[test]
fn test_split() {
    let results: Vec<Vec<u8>> =
        h264_annexb_split(&[0, 0, 0, 1, 9, 10, 10, 0, 0, 0, 1, 3, 20, 0, 0, 0, 1, 99, 99])
            .collect();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], &[9, 10, 10]);
    assert_eq!(results[1], &[3, 20]);
    assert_eq!(results[2], &[99, 99]);

    let results: Vec<Vec<u8>> =
        h264_annexb_split(&[0, 0, 1, 9, 10, 10, 0, 0, 1, 3, 20, 0, 0, 1, 99, 99]).collect();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], &[9, 10, 10]);
    assert_eq!(results[1], &[3, 20]);
    assert_eq!(results[2], &[99, 99]);

    let results: Vec<Vec<u8>> = h264_annexb_split(&[]).collect();
    assert_eq!(results.len(), 0);
}
