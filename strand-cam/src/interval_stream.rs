// From https://github.com/tokio-rs/tokio/discussions/3341

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::stream::Stream;
use tokio::time::{Instant, Interval};

pub(crate) struct MyInterval {
    pub(crate) int: Interval,
}

impl Stream for MyInterval {
    type Item = Instant;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Instant>> {
        self.int.poll_tick(cx).map(|inner| Some(inner))
    }
}
