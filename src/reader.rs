use std::{pin::Pin, task::Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures::{AsyncRead, Stream};

use crate::error::Error;

const LENGTH_MARKER_SIZE: usize = std::mem::size_of::<u16>();

pub(crate) struct Reader<R: AsyncRead> {
    stream: R,
    buffer: BytesMut,
}

impl<R: AsyncRead> Reader<R> {
    #[cfg(test)]
    pub(crate) fn new(stream: R) -> Self {
        Self {
            stream,
            buffer: BytesMut::new(),
        }
    }

    // If there's a full frame in the bufer, it's length is returned.
    fn have_full_frame(&self) -> Result<Option<usize>, Error> {
        let bytes_in_buffer = self.buffer.remaining();
        if bytes_in_buffer < LENGTH_MARKER_SIZE {
            return Ok(None);
        }

        let data_length = u16::from_le_bytes(
            self.buffer[0..LENGTH_MARKER_SIZE]
                .try_into()
                .map_err(|_| Error::IncorrectFrameLength)?,
        ) as usize;

        if bytes_in_buffer < LENGTH_MARKER_SIZE + data_length {
            return Ok(None);
        }

        Ok(Some(LENGTH_MARKER_SIZE + data_length))
    }
}

impl<R> Stream for Reader<R>
where
    R: AsyncRead + Unpin,
{
    type Item = Bytes;

    // TODO: Add UTs for all paths
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut intermediate_buffer = [0; 128];
        let mut reader_mut = self.as_mut();
        let frame_length = loop {
            match reader_mut.have_full_frame() {
                Ok(maybe_length) => match maybe_length {
                    Some(frame_length) => break frame_length,
                    None => {
                        // TODO: Borrow checker doesn't like using `reader_mut.buffer` directly.
                        match Pin::new(&mut reader_mut.stream)
                            .poll_read(cx, &mut intermediate_buffer)
                        {
                            Poll::Ready(result) => match result {
                                Ok(count) => {
                                    // For testing purposes assume that when the stream is empty
                                    // we finish processing. In production, we'll keep waiting
                                    // for more data to arrive.
                                    #[cfg(test)]
                                    if count == 0 {
                                        return Poll::Ready(None);
                                    }

                                    reader_mut
                                        .buffer
                                        .extend_from_slice(&intermediate_buffer[0..count])
                                }
                                Err(err) => panic!("error on Reader::poll_read(): {}", err),
                            },
                            Poll::Pending => return Poll::Pending,
                        }
                    }
                },
                Err(err) => panic!("error on have_full_frame(): {}", err),
            }
        };

        let mut frame_data = reader_mut.buffer.split_to(frame_length);
        let _ = frame_data.split_to(LENGTH_MARKER_SIZE);

        Poll::Ready(Some(frame_data.freeze()))
    }
}
