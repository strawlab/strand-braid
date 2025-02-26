use std::{io::Read, time::Duration};

use crate::Result;

use winnow::{
    ascii::{dec_uint, digit1, line_ending},
    combinator::{eof, opt, seq, terminated, trace},
    error::{ContextError, ErrMode, InputError},
    prelude::*,
    token::{take, take_until, take_while},
    BStr,
};

fn parse_digits<'s>(input: &mut &'s BStr) -> ModalResult<u64> {
    trace("parse_digits", move |input: &mut &'s BStr| {
        digit1
            .parse_to()
            .parse_next(input)
            .map_err(|_e: ErrMode<InputError<&'s BStr>>| ErrMode::Cut(ContextError::new()))
    })
    .parse_next(input)
}

fn parse_duration(input: &mut &BStr) -> ModalResult<Duration> {
    trace("parse_duration", move |input: &mut &BStr| {
        let (hours, _, minutes, _, seconds, _, millis) = (
            take(2usize),
            ':',
            take(2usize),
            ':',
            take(2usize),
            ',',
            take(3usize),
        )
            .parse_next(input)?;

        let hours: u64 = parse_digits(&mut hours.into())?;
        let minutes: u64 = parse_digits(&mut minutes.into())?;
        let seconds: u64 = parse_digits(&mut seconds.into())?;
        let millis: u64 = parse_digits(&mut millis.into())?;

        let minutes = hours * 60 + minutes;
        let secs = 60 * minutes + seconds;
        let nanos = millis * 1_000_000;
        Ok(Duration::new(secs, nanos.try_into().unwrap()))
    })
    .parse_next(input)
}

#[rustfmt::skip]
#[derive(Debug)]
pub(crate) struct Stanza {
    pub(crate) _count: usize,
    pub(crate) _start: std::time::Duration,
    pub(crate) _stop: std::time::Duration,
    pub(crate) lines: String,
}

fn parse_stanza(input: &mut &BStr) -> ModalResult<Stanza> {
    trace("parse_stanza", move |input: &mut &BStr| {
        let mut num = dec_uint::<_, usize, ContextError>;

        // first line: count
        let count_res: winnow::Result<(usize,)> = seq!(num, _: line_ending).parse_next(input);
        let count = count_res.unwrap().0;

        // "00:00:00,100 --> 00:00:00,210"
        let start_stop_res: ModalResult<(Duration, Duration)> =
            seq!(parse_duration, _: " --> ", parse_duration, _: line_ending).parse_next(input);
        let (start, stop) = start_stop_res?;

        // TODO: match against two `line_ending`s (rather than only '\n')
        let till_newlines = take_until(0.., "\n\n");

        let res: ModalResult<&[u8]> = if let Some(lines0) = opt(till_newlines).parse_next(input)? {
            // Clear one trailing newline. (Leave other as stanza seperator.)
            "\n".parse_next(input)?;
            Ok(lines0)
        } else {
            // We reached EOF
            terminated(take_while(0.., |_| true), eof).parse_next(input)
        };
        let lines0 = res?;
        let lines = String::from_utf8(lines0.to_vec()).unwrap();

        Ok(Stanza {
            _count: count,
            _start: start,
            _stop: stop,
            lines,
        })
    })
    .parse_next(input)
}

fn parse_stanzas(input: &mut &BStr) -> ModalResult<Vec<Stanza>> {
    trace("parse_stanzas", move |input: &mut &BStr| {
        let mut result = vec![];
        loop {
            if opt(eof).parse_next(input)?.is_some() {
                break;
            } else {
                let x = parse_stanza.parse_next(input)?;
                result.push(x);
                if opt(eof).parse_next(input)?.is_some() {
                    break;
                } else {
                    line_ending.parse_next(input)?;
                }
            }
        }
        Ok(result)
    })
    .parse_next(input)
}

pub(crate) fn read_srt_file(p: &std::path::Path) -> Result<Vec<Stanza>> {
    let mut fd = std::fs::File::open(p)?;
    let mut buf = Vec::new();
    fd.read_to_end(&mut buf)?;
    let mut buf_bstr: &BStr = buf.as_slice().into();

    let stanzas: Vec<Stanza> = parse_stanzas
        .parse_next(&mut buf_bstr)
        .map_err(|_e| crate::Error::SrtParseError)?;

    Ok(stanzas)
}

#[cfg(test)]
mod test {
    use super::*;

    const B0: &[u8] = b"";

    const B1: &[u8] = br#"1
00:00:00,000 --> 00:00:00,040
{"frame_cnt":1,"timestamp":"2024-11-21T21:04:19.534412+01:00"}
"#;

    const B2A: &[u8] = br#"1
00:00:00,000 --> 00:00:00,040
{"frame_cnt":1,"timestamp":"2024-11-21T21:04:19.534412+01:00"}

2
00:00:00,040 --> 00:00:00,080
{"frame_cnt":2,"timestamp":"2024-11-21T21:04:19.552417+01:00"}"#;

    const B2B: &[u8] = br#"1
00:00:00,000 --> 00:00:00,040
{"frame_cnt":1,"timestamp":"2024-11-21T21:04:19.534412+01:00"}

2
00:00:00,040 --> 00:00:00,080
{"frame_cnt":2,"timestamp":"2024-11-21T21:04:19.552417+01:00"}

"#;

    const B3A: &[u8] = br#"1
00:00:00,000 --> 00:00:00,040
{"frame_cnt":1,"timestamp":"2024-11-21T21:04:19.534412+01:00"}

2
00:00:00,040 --> 00:00:00,080
{"frame_cnt":2,"timestamp":"2024-11-21T21:04:19.552417+01:00"}

3
00:00:00,080 --> 00:00:00,120
{"frame_cnt":3,"timestamp":"2024-11-21T21:04:19.563575+01:00"}"#;

    const B3B: &[u8] = br#"1
00:00:00,000 --> 00:00:00,040
{"frame_cnt":1,"timestamp":"2024-11-21T21:04:19.534412+01:00"}

2
00:00:00,040 --> 00:00:00,080
{"frame_cnt":2,"timestamp":"2024-11-21T21:04:19.552417+01:00"}

3
00:00:00,080 --> 00:00:00,120
{"frame_cnt":3,"timestamp":"2024-11-21T21:04:19.563575+01:00"}
"#;

    #[test]
    fn test_parse() {
        for (sz, in_b3) in [(0, B0), (1, B1), (2, B2A), (2, B2B), (3, B3A), (3, B3B)] {
            println!(
                "testing size {sz} with value:\n{:?}",
                String::from_utf8_lossy(in_b3)
            );
            let b3 = parse_stanzas.parse_next(&mut in_b3.into()).unwrap();
            assert_eq!(b3.len(), sz);
        }
    }
}

#[cfg(test)]
mod test_duration {
    use super::*;

    trait Srt {
        fn srt(&self) -> String;
    }

    impl Srt for Duration {
        fn srt(&self) -> String {
            // from https://en.wikipedia.org/wiki/SubRip :
            // "hours:minutes:seconds,milliseconds with time units fixed to two
            // zero-padded digits and fractions fixed to three zero-padded digits
            // (00:00:00,000). The fractional separator used is the comma, since the
            // program was written in France."
            let total_secs = self.as_secs();
            let hours = total_secs / (60 * 60);
            let minutes = (total_secs % (60 * 60)) / 60;
            let seconds = total_secs % 60;
            debug_assert_eq!(total_secs, hours * 60 * 60 + minutes * 60 + seconds);
            let millis = self.subsec_millis();
            format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
        }
    }
    #[test]
    fn test_duration_roundtrip() {
        for (h, m, s, ms) in [(1, 2, 3, 4), (3, 2, 1, 0), (10, 9, 8, 999)] {
            let m = h * 60 + m;
            let s = m * 60 + s;
            let ms = s * 1000 + ms;
            let dur = Duration::from_millis(ms);
            let dur_str = dur.srt();
            let dur_bytes: &BStr = dur_str.as_str().into();
            let parsed = trace("parse_duration", parse_duration).parse(dur_bytes);
            let parsed = parsed.unwrap();
            assert_eq!(dur, parsed);
        }
    }
}
