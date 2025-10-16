// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Allows silently swallowing `UnexpectedEof` errors when reading CSV files.
use std::io::ErrorKind;

/// Wrap an iterator in this to silently swallow `UnexpectedEof` errors.
///
/// Often when we are saving CSV files, they may be abruptly terminated when the
/// program quits unexpectedly or the disk is full. While the problem should be
/// solved elsewhere, the reality is that such corrupt CSV files exist and we
/// want to parse them.
pub struct TerminateEarlyOnUnexpectedEof<I, T>
where
    I: Iterator<Item = Result<T, csv::Error>>,
{
    inner: I,
}

impl<I, T> TerminateEarlyOnUnexpectedEof<I, T>
where
    I: Iterator<Item = Result<T, csv::Error>>,
{
    /// create a TerminateEarlyOnUnexpectedEof
    pub fn new(inner: I) -> Self {
        Self { inner }
    }

    /// unwrap a TerminateEarlyOnUnexpectedEof
    pub fn into_inner(self) -> I {
        self.inner
    }
}

impl<I, T> Iterator for TerminateEarlyOnUnexpectedEof<I, T>
where
    I: Iterator<Item = Result<T, csv::Error>>,
{
    type Item = Result<T, csv::Error>;
    fn next(&mut self) -> std::option::Option<<Self as Iterator>::Item> {
        match self.inner.next() {
            Some(Ok(item)) => Some(Ok(item)),
            None => None,
            Some(Err(e)) => match is_early_eof(&e) {
                true => None,
                false => Some(Err(e)),
            },
        }
    }
}

/// check a `csv::Error` and return `true` iff it is an UnexpectedEof error
fn is_early_eof(e: &csv::Error) -> bool {
    if let csv::ErrorKind::Io(io_err) = e.kind() {
        if let ErrorKind::UnexpectedEof = io_err.kind() {
            return true;
        }
    }
    false
}

/// A trait to wrap an Iterator and terminate without error on UnexpectedEof
pub trait EarlyEofOk<I, T>
where
    I: Iterator<Item = Result<T, csv::Error>>,
{
    fn early_eof_ok(self) -> TerminateEarlyOnUnexpectedEof<I, T>;
}

impl<I, T> EarlyEofOk<I, T> for I
where
    I: Iterator<Item = Result<T, csv::Error>>,
{
    fn early_eof_ok(self) -> TerminateEarlyOnUnexpectedEof<I, T> {
        TerminateEarlyOnUnexpectedEof::new(self)
    }
}
