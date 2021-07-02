//! Outputs a stream of contiguous elements, even when some missing from input.

use std::pin::Pin;

use futures::{
    stream::Stream,
    task::{Context, Poll},
};

use pin_project::pin_project;

/// An element with a number.
pub(crate) trait Numbered: Unpin {
    fn number(&self) -> u64;
    fn new_empty(number: u64) -> Self;
}

/// Convert a monotonically increasing stream to a contiguous stream.
///
/// The input stream can have gaps. This finishes immediately if the input
/// stream is not monotonically increasing. This means that repeated numbers or
/// backwards numbers cannot occur.
///
/// The resulting stream will have no gaps and any missing element will be
/// created with `T::new_empty()`.
pub(crate) fn make_contiguous<St, T>(stream: St) -> NumberedContiguous<St, T>
where
    St: Stream<Item = T>,
    T: Numbered,
{
    NumberedContiguous {
        stream,
        previous: None,
        stage: Some(ContigStage::WaitingForNext),
        pending: None,
    }
}

/// Implements the stream of contiguous elements.
#[pin_project]
pub(crate) struct NumberedContiguous<St, T> {
    #[pin]
    stream: St,
    previous: Option<u64>,
    stage: Option<ContigStage<T>>,
    #[pin]
    pending: Option<T>,
}

impl<St, T> NumberedContiguous<St, T>
where
    St: Stream<Item = T>,
    T: Numbered,
{
    pub(crate) fn inner(self) -> St {
        self.stream
    }
}

enum ContigStage<T> {
    CatchingUp((u64, T)),
    WaitingForNext,
}

impl<St, T> Stream for NumberedContiguous<St, T>
where
    St: Stream<Item = T>,
    T: Numbered,
{
    type Item = T;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let mut this = self.project();

        debug_assert!(this.stage.is_some(), "stage is unknown");

        let current_stage = { this.stage.take().unwrap() };

        use ContigStage::*;
        match current_stage {
            CatchingUp((diff, item)) => {
                let item_number = item.number();

                let previous = { this.previous.as_mut().take().unwrap() };

                if diff == 1 {
                    // The new item is already contiguous.
                    *this.previous = Some(item_number);
                    *this.stage = Some(WaitingForNext);
                    return Poll::Ready(Some(item));
                } else {
                    let next = *previous + 1;
                    let next_item = Self::Item::new_empty(next);

                    *this.previous = Some(next);
                    *this.stage = Some(CatchingUp((diff - 1, item)));
                    return Poll::Ready(Some(next_item));
                }
            }
            WaitingForNext => {
                // ensure that we have a pending item to work with, return if not.
                if this.pending.is_none() {
                    let item = match this.stream.poll_next(cx) {
                        Poll::Pending => {
                            *this.stage = Some(current_stage);
                            return Poll::Pending;
                        }
                        Poll::Ready(Some(e)) => e,
                        Poll::Ready(None) => {
                            *this.stage = Some(WaitingForNext);
                            return Poll::Ready(None);
                        }
                    };
                    this.pending.set(Some(item));
                }

                // The following unwrap cannot fail because of above.
                let item = this.pending.take().unwrap();
                let item_number = item.number();
                match this.previous {
                    None => {
                        // This is the first item
                        *this.previous = Some(item_number);
                        *this.stage = Some(WaitingForNext);
                        return Poll::Ready(Some(item));
                    }
                    Some(previous) => {
                        // This is a new item and we had a previous item.
                        if !(item_number > *previous) {
                            // items not monotonically increasing - stop
                            return Poll::Ready(None);
                        }
                        let diff = item_number - *previous;
                        if diff == 1 {
                            // The new item is already contiguous.
                            *this.previous = Some(item_number);
                            *this.stage = Some(WaitingForNext);
                            return Poll::Ready(Some(item));
                        } else {
                            let next = *previous + 1;
                            let next_item = Self::Item::new_empty(next);

                            *this.previous = Some(next);
                            *this.stage = Some(CatchingUp((diff - 1, item)));
                            return Poll::Ready(Some(next_item));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use futures::future;
    use futures::stream::{self, StreamExt};

    use super::*;

    #[derive(Debug, Clone)]
    struct Blarg {
        current: u64,
    }

    trait VecNums {
        fn from_nums(self) -> Vec<Blarg>;
    }

    impl VecNums for Vec<u64> {
        fn from_nums(self) -> Vec<Blarg> {
            self.into_iter().map(|current| Blarg { current }).collect()
        }
    }

    trait VecBlarg {
        fn to_nums(self) -> Vec<u64>;
    }

    impl VecBlarg for Vec<Blarg> {
        fn to_nums(self) -> Vec<u64> {
            self.into_iter().map(|x| x.current).collect()
        }
    }

    impl Numbered for Blarg {
        fn number(&self) -> u64 {
            self.current
        }
        fn new_empty(number: u64) -> Self {
            Self { current: number }
        }
    }

    fn check_contig(inputs: Vec<u64>, expected: Vec<u64>) {
        let contig = make_contiguous(stream::iter(inputs.from_nums()));
        let actual: Vec<_> = futures::executor::block_on(contig.collect());
        let actual: Vec<u64> = actual.to_nums();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_make_contiguous() {
        let inputs = vec![0, 1, 2, 4, 10];
        let expected = (0..=10).collect::<Vec<_>>();
        check_contig(inputs, expected);

        let inputs = vec![];
        let expected = vec![];
        check_contig(inputs, expected);

        let inputs = vec![0];
        let expected = vec![0];
        check_contig(inputs, expected);

        let inputs = vec![1];
        let expected = vec![1];
        check_contig(inputs, expected);

        let inputs = vec![1, 2];
        let expected = vec![1, 2];
        check_contig(inputs, expected);

        let inputs = vec![1, 2, 3];
        let expected = vec![1, 2, 3];
        check_contig(inputs, expected);

        let inputs = vec![1, 2, 3, 10];
        let expected = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        check_contig(inputs, expected);
    }

    #[test]
    fn test_make_contiguous_backwards() {
        let inputs = vec![10, 9];
        let expected = vec![10];
        check_contig(inputs, expected);
    }

    #[test]
    fn test_make_contiguous_repeat() {
        let inputs = vec![10, 10];
        let expected = vec![10];
        check_contig(inputs, expected);
    }

    #[test]
    fn test_async_stream_ops() {
        let stream = stream::iter(1..=10);
        let evens = stream.filter_map(|x| {
            let ret = if x % 2 == 0 { Some(x + 1) } else { None };
            future::ready(ret)
        });

        let result: Vec<_> = futures::executor::block_on(evens.collect());
        assert_eq!(vec![3, 5, 7, 9, 11], result);
    }
}
