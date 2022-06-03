use std::{pin::Pin, task::Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures::{AsyncRead, Stream};

use crate::error::Error;

const LENGTH_MARKER_SIZE: usize = std::mem::size_of::<u16>();
#[cfg(test)]
const BUFFER_SIZE: usize = 8;
#[cfg(not(test))]
const BUFFER_SIZE: usize = 1024;

pub(crate) struct FrameReader<R: AsyncRead> {
    stream: R,
    buffer: BytesMut,
}

impl<R: AsyncRead> FrameReader<R> {
    #[cfg(test)]
    pub(crate) fn new(stream: R) -> Self {
        Self {
            stream,
            buffer: BytesMut::new(),
        }
    }
}

fn length_delimited_frame(buffer: &mut BytesMut) -> Result<Option<BytesMut>, Error> {
    let bytes_in_buffer = buffer.remaining();
    if bytes_in_buffer < LENGTH_MARKER_SIZE {
        return Ok(None);
    }
    let data_length = u16::from_le_bytes(
        buffer[0..LENGTH_MARKER_SIZE]
            .try_into()
            .map_err(|_| Error::IncorrectFrameLength)?,
    ) as usize;

    let end = LENGTH_MARKER_SIZE + data_length;

    if bytes_in_buffer < end {
        return Ok(None);
    }

    let mut full_frame = buffer.split_to(end);
    let _ = full_frame.get_u16_le();

    Ok(Some(full_frame))
}

impl<R> Stream for FrameReader<R>
where
    R: AsyncRead + Unpin,
{
    // TODO: Ultimately, this should become Result<Bytes>.
    type Item = Bytes;

    // TODO: Add UTs for all paths
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let FrameReader {
            ref mut stream,
            ref mut buffer,
        } = self.get_mut();
        loop {
            match length_delimited_frame(buffer) {
                Ok(result) => match result {
                    Some(frame) => return Poll::Ready(Some(frame.freeze())),
                    None => {
                        let start = buffer.len();
                        let end = start + BUFFER_SIZE;
                        buffer.resize(end, 0xBA);

                        match Pin::new(&mut *stream).poll_read(cx, &mut buffer[start..end]) {
                            Poll::Ready(result) => match result {
                                Ok(bytes_read) => {
                                    buffer.truncate(start + bytes_read);
                                    dbg!(&buffer);

                                    // For testing purposes assume that when the stream is empty
                                    // we finish processing. In production, we'll keep waiting
                                    // for more data to arrive.
                                    #[cfg(test)]
                                    if bytes_read == 0 {
                                        return Poll::Ready(None);
                                    }
                                }
                                Err(err) => panic!("poll_read() failed: {}", err),
                            },
                            Poll::Pending => return Poll::Pending,
                        }
                    }
                },
                Err(err) => panic!("length_delimited_frame() failed: {}", err),
            }
        }
    }
}
