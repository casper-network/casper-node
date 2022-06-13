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

use futures::AsyncRead;

use crate::try_ready;

/// The read end of a pipe.
#[derive(Debug)]
pub struct ReadEnd {
    /// Buffer containing read data.
    buffer: Arc<Mutex<PipeInner>>,
}

/// The write end of a pipe.
#[derive(Debug)]
pub struct WriteEnd {
    /// Buffer containing write data.
    buffer: Arc<Mutex<PipeInner>>,
}

/// Innards of a pipe.
#[derive(Debug)]
struct PipeInner {
    /// Buffer for data currently in the pipe.
    buffer: Option<VecDeque<u8>>,
    /// Waker for the reader of the pipe.
    read_waker: Option<Waker>,
}

/// Acquire a guard on a buffer mutex.
fn acquire_lock(buffer: &mut Arc<Mutex<PipeInner>>) -> io::Result<MutexGuard<PipeInner>> {
    match buffer.lock() {
        Ok(guard) => Ok(guard),
        Err(poisoned) => Err(io::Error::new(io::ErrorKind::Other, poisoned.to_string())),
    }
}

impl Drop for ReadEnd {
    fn drop(&mut self) {
        let guard = acquire_lock(&mut self.buffer)
            .expect("could not acquire lock during drop of `ReadEnd`");

        guard.buffer.take();

        if let Some(waker) = guard.read_waker.take() {
            waker.wake();
        }
    }
}

impl Drop for WriteEnd {
    fn drop(&mut self) {
        let guard = acquire_lock(&mut self.buffer)
            .expect("could not acquire lock during drop of `ReadEnd`");

        guard.buffer.take();

        if let Some(waker) = guard.read_waker.take() {
            waker.wake();
        }
    }
}

impl io::Read for ReadEnd {
    fn read(&mut self, dest: &mut [u8]) -> io::Result<usize> {
        let mut guard = acquire_lock(&mut self.buffer)?;

        match *guard {
            Some(ref mut buffer) => {
                let to_read = buffer.len().min(dest.len());

                // This is a bit ugly and probably slow, but will have to do for now :(
                for (idx, c) in buffer.drain(0..to_read).enumerate() {
                    dest[idx] = c;
                }

                Ok(to_read)
            }
            // On a closed channel, simply return 0 bytes read.
            None => Ok(0),
        }
    }
}

impl io::Write for WriteEnd {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = acquire_lock(&mut self.buffer)?;

        match *guard {
            Some(ref mut buffer) => {
                buffer.extend(buf);
                Ok(buf.len())
            }
            None => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "internal pipe closed",
            )),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let guard = acquire_lock(&mut self.buffer)?;

        if guard.is_none() {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "internal pipe closed",
            ))
        } else {
            Ok(())
        }
    }
}

impl AsyncRead for ReadEnd {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        dest: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut guard = try_ready!(acquire_lock(&mut self.buffer));

        match *guard {
            Some(ref mut buffer) => {
                if buffer.is_empty() {
                    // TODO: Register waker.
                    Poll::Pending
                } else {
                    let to_read = buffer.len().min(dest.len());

                    // This is a bit ugly and probably slow, but will have to do for now :(
                    for (idx, c) in buffer.drain(0..to_read).enumerate() {
                        dest[idx] = c;
                    }

                    Poll::Ready(Ok(to_read))
                }
            }
            None => Poll::Ready(Ok(0)),
        }
    }
}

/// Creates a new synchronous pipe.
///
/// The resulting pipe will write all data into an infinitely growing memory buffer. All writes will
/// succeed, unless the pipe is closed. Reads will immediately return as much data as is available.
///
/// Dropping either end of the pipe will close the other end.
pub(crate) fn pipe() -> (ReadEnd, WriteEnd) {
    let buffer: Arc<Mutex<_>> = Default::default();
    let read_end = ReadEnd {
        buffer: buffer.clone(),
    };
    let write_end = WriteEnd { buffer };
    (read_end, write_end)
}

#[cfg(test)]
mod tests {
    use super::pipe;

    #[test]
    fn sync_pipe_works() {
        let (mut read_end, mut write_end) = pipe();

        // let write_end
    }
}
