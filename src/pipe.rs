//! IO pipes for testing.
//!
//! A pipe writes to an infinite memory buffer and can be used to test async read/write IO.

use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
    task::{Context, Poll, Waker},
};

use futures::{AsyncRead, AsyncWrite};

use crate::try_ready;

/// The read end of a pipe.
#[derive(Debug)]
pub struct ReadEnd {
    /// Buffer containing read data.
    inner: Arc<Mutex<PipeInner>>,
}

/// The write end of a pipe.
#[derive(Debug)]
pub struct WriteEnd {
    /// Buffer containing write data.
    inner: Arc<Mutex<PipeInner>>,
}

/// Innards of a pipe.
#[derive(Debug, Default)]
struct PipeInner {
    /// Buffer for data currently in the pipe.
    buffer: VecDeque<u8>,
    /// Whether or not the pipe has been closed.
    closed: bool,
    /// Waker for the reader of the pipe.
    read_waker: Option<Waker>,
}

/// Acquire a guard on a buffer mutex.
fn acquire_lock(inner: &mut Arc<Mutex<PipeInner>>) -> io::Result<MutexGuard<PipeInner>> {
    match inner.lock() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => Err(io::Error::new(io::ErrorKind::Other, poisoned.to_string())),
    }
}

impl Drop for ReadEnd {
    fn drop(&mut self) {
        let mut guard =
            acquire_lock(&mut self.inner).expect("could not acquire lock during drop of `ReadEnd`");

        guard.closed = true;

        if let Some(waker) = guard.read_waker.take() {
            waker.wake();
        }
    }
}

impl Drop for WriteEnd {
    fn drop(&mut self) {
        let mut guard =
            acquire_lock(&mut self.inner).expect("could not acquire lock during drop of `ReadEnd`");

        guard.closed = true;

        if let Some(waker) = guard.read_waker.take() {
            waker.wake();
        }
    }
}

impl AsyncRead for ReadEnd {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        dest: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut inner = try_ready!(acquire_lock(&mut self.inner));

        if inner.buffer.is_empty() {
            if inner.closed {
                Poll::Ready(Ok(0))
            } else {
                inner.read_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        } else {
            let to_read = inner.buffer.len().min(dest.len());

            // This is a bit ugly and probably slow, but will have to do for now :(
            for (idx, c) in inner.buffer.drain(0..to_read).enumerate() {
                dest[idx] = c;
            }

            Poll::Ready(Ok(to_read))
        }
    }
}

impl AsyncWrite for WriteEnd {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        source: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut guard = try_ready!(acquire_lock(&mut self.get_mut().inner));

        if guard.closed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "async testing pipe closed",
            )));
        }

        guard.buffer.extend(source);

        if let Some(waker) = guard.read_waker.take() {
            waker.wake();
        }

        Poll::Ready(Ok(source.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Poll will never have any effect, so we do not need to wake anyone.

        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut guard = try_ready!(acquire_lock(&mut self.get_mut().inner));

        guard.closed = true;
        if let Some(waker) = guard.read_waker.take() {
            waker.wake();
        }

        Poll::Ready(Ok(()))
    }
}

/// Creates a new asynchronous pipe.
///
/// The resulting pipe will write all data into an infinitely growing memory buffer. All writes will
/// succeed, unless the pipe is closed. Reads will immediately return as much data as is available
/// and be properly woken up if more data is required.
///
/// Dropping either end of the pipe will close it, causing writes to return broken pipe errors and
/// reads to return successful 0-byte reads.
pub(crate) fn pipe() -> (WriteEnd, ReadEnd) {
    let inner: Arc<Mutex<_>> = Default::default();
    let read_end = ReadEnd {
        inner: inner.clone(),
    };
    let write_end = WriteEnd { inner };
    (write_end, read_end)
}

#[cfg(test)]
mod tests {
    use futures::{AsyncReadExt, AsyncWriteExt, FutureExt};

    use super::pipe;

    #[test]
    fn async_pipe_works() {
        let (mut write_end, mut read_end) = pipe();

        assert!(read_end
            .read_to_end(&mut Vec::new())
            .now_or_never()
            .is_none());

        write_end.write_all(b"one").now_or_never().unwrap().unwrap();
        write_end.write_all(b"two").now_or_never().unwrap().unwrap();

        let mut buf = [0; 5];
        read_end
            .read_exact(&mut buf)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert_eq!(&buf, b"onetw");

        let mut remainder: Vec<u8> = Vec::new();

        write_end
            .write_all(b"three")
            .now_or_never()
            .unwrap()
            .unwrap();

        write_end.close().now_or_never().unwrap().unwrap();

        read_end
            .read_to_end(&mut remainder)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert_eq!(remainder, b"othree");
    }
}
