use std::{num::NonZeroUsize, pin::Pin};

use futures::{
    task::{Context, Poll},
    {Stream, StreamExt},
};

use crate::event::{EventArray, EventContainer};

const DEFAULT_CAPACITY: usize = 4096;

/// A stream combinator aimed at improving the performance of event streams under load.
///
/// This is similar in spirit to `StreamExt::ready_chunks`, but built specifically `EventArray`.
/// The more general `FoldReady` is left as an exercise to the reader.
pub struct ReadyArrays<T> {
    inner: T,
    enqueued: Vec<EventArray>,
    /// Distinct from `enqueued.len()`, counts the number of total `Event`
    /// instances in all sub-arrays.
    enqueued_size: usize,
    enqueued_limit: usize,
}

impl<T> ReadyArrays<T>
where
    T: Stream<Item = EventArray> + Unpin,
{
    /// Create a new `ReadyArrays` by wrapping an event array stream.
    pub fn new(inner: T) -> Self {
        Self::with_capacity(inner, NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())
    }

    /// Create a new `ReadyArrays` with a specified capacity.
    ///
    /// The specified capacity is a soft limit, and chunks may be returned that contain more than
    /// that number of items.
    pub fn with_capacity(inner: T, capacity: NonZeroUsize) -> Self {
        Self {
            inner,
            enqueued: Vec::with_capacity(capacity.get()),
            enqueued_size: 0,
            enqueued_limit: capacity.get(),
        }
    }

    fn flush(&mut self) -> Vec<EventArray> {
        let mut arrays = Vec::with_capacity(self.enqueued_limit);
        std::mem::swap(&mut arrays, &mut self.enqueued);
        self.enqueued_size = 0;
        arrays
    }
}

impl<T> Stream for ReadyArrays<T>
where
    T: Stream<Item = EventArray> + Unpin,
{
    type Item = Vec<EventArray>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.inner.poll_next_unpin(cx) {
                Poll::Ready(Some(array)) => {
                    self.enqueued_size += array.len();
                    self.enqueued.push(array);
                    if self.enqueued_size >= self.enqueued_limit {
                        return Poll::Ready(Some(self.flush()));
                    }
                }
                Poll::Ready(None) | Poll::Pending => {
                    // When the inner stream is empty or signals pending flush
                    // everything we've got enqueued here.
                    if !self.enqueued.is_empty() {
                        return Poll::Ready(Some(self.flush()));
                    } else {
                        return Poll::Ready(None);
                    }
                }
            }
        }
    }
}
