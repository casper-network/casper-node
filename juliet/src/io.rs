//! `juliet` IO layer
//!
//! The IO layer combines a lower-level transport like a TCP Stream with the
//! [`JulietProtocol`](crate::juliet::JulietProtocol) protocol implementation and some memory buffer
//! to provide a working high-level transport for juliet messages. It allows users of this layer to
//! send messages across over multiple channels, without having to worry about frame multiplexing or
//! request limits.
//!
//! The layer is designed to run in its own task, with handles to allow sending messages in, or
//! receiving them as they arrive.

use std::{
    collections::{HashMap, VecDeque},
    io,
};

use bytes::{Buf, BytesMut};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc::Receiver,
};

use crate::{
    header::Header,
    protocol::{CompletedRead, FrameIter, JulietProtocol, OutgoingFrame, OutgoingMessage},
    ChannelId, Outcome,
};

struct QueuedRequest {
    channel: ChannelId,
    message: OutgoingMessage,
}

/// [`IoCore`] error.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Failed to read from underlying reader.
    #[error("read failed")]
    ReadFailed(#[source] io::Error),
    /// Failed to write using underlying writer.
    #[error("write failed")]
    WriteFailed(#[source] io::Error),
}

pub struct IoCore<const N: usize, R, W> {
    /// The actual protocol state.
    juliet: JulietProtocol<N>,

    /// Underlying transport, reader.
    reader: R,
    /// Underlying transport, writer.
    writer: W,
    /// Read buffer for incoming data.
    buffer: BytesMut,

    /// The message that is in the process of being sent.
    current_message: Option<FrameIter>,
    /// The frame in the process of being sent.
    current_frame: Option<OutgoingFrame>,
    run_queue: VecDeque<()>,
    flagmap: [(); N],
    counter: [(); N],
    req_store: [HashMap<u16, ()>; N],
    resp_store: [HashMap<u16, ()>; N],
    request_input: Receiver<QueuedRequest>,
    _confirmation_queue: (), // ?
}

impl<const N: usize, R, W> IoCore<N, R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub async fn run(mut self, read_buffer_size: usize) -> Result<(), CoreError> {
        let mut bytes_until_next_parse = Header::SIZE;

        loop {
            // Note: There is a world in which we find a way to reuse some of the futures instead
            //       of recreating them with every loop iteration, but I was not able to convince
            //       the borrow checker yet.

            tokio::select! {
                biased;  // We do not need the bias, but we want to avoid randomness overhead.

                // New requests coming in from clients:
                new_request = self.request_input.recv() => {
                    drop(new_request); // TODO: Sort new request into queues.
                }

                // Writing outgoing data:
                write_result = self.writer.write_all_buf(self.current_frame.as_mut().unwrap())
                    , if self.current_frame.is_some() => {
                    write_result.map_err(CoreError::WriteFailed)?;

                    self.advance_write();
                }

                // Reading incoming data:
                read_result = read_atleast_bytesmut(&mut self.reader, &mut self.buffer, bytes_until_next_parse) => {
                    let bytes_read = read_result.map_err(CoreError::ReadFailed)?;

                    bytes_until_next_parse = bytes_until_next_parse.saturating_sub(bytes_read);

                    if bytes_until_next_parse == 0 {
                        match self.juliet.process_incoming(&mut self.buffer) {
                            Outcome::Incomplete(n) => {
                                // Simply reset how many bytes we need until the next parse.
                                bytes_until_next_parse = n.get() as usize;
                            },
                            Outcome::Fatal(err) => {
                                self.handle_fatal_read_err(err)
                            },
                            Outcome::Success(successful_read) => {
                                self.handle_completed_read(successful_read)
                            },
                        }
                    }

                    if bytes_read == 0 {
                        // Remote peer hung up.
                        return Ok(());
                    }
                }
            }
        }
    }

    fn handle_completed_read(&mut self, read: CompletedRead) {
        match read {
            CompletedRead::ErrorReceived { header, data } => todo!(),
            CompletedRead::NewRequest { id, payload } => todo!(),
            CompletedRead::ReceivedResponse { id, payload } => todo!(),
            CompletedRead::RequestCancellation { id } => todo!(),
            CompletedRead::ResponseCancellation { id } => todo!(),
        }
    }

    fn handle_fatal_read_err(&mut self, err: OutgoingMessage) {
        todo!()
    }

    fn next_frame(&mut self, max_frame_size: usize) {
        // If we still have frame data, return.
        if self
            .current_frame
            .as_ref()
            .map(Buf::has_remaining)
            .unwrap_or(false)
        {
            return;
        } else {
            // Reset frame to be sure.
            self.current_frame = None;
        }

        // At this point, we need to fetch another frame. This is only possible if we have a message
        // to pull frames from.
        loop {
            if let Some(ref mut current_message) = self.current_message {
                match current_message.next(self.juliet.max_frame_size()) {
                    Some(frame) => {
                        self.current_frame = Some(frame);
                        // Successful, current message had another frame.
                    }
                    None => {
                        // There is no additional frame from the current message.
                        self.current_message = None;
                    }
                }

                // We neither have a message nor a frame, time to look into the queue.
                let next_item = self.run_queue.pop_back();
            }
        }
    }

    fn advance_write(&mut self) {
        // Discard frame if finished.
        if let Some(ref frame) = self.current_frame {
            if frame.remaining() == 0 {
                self.current_frame = None;
            } else {
                // We still have a frame to finish.
                return;
            }
        }

        if let Some(ref message) = self.current_message {}

        // Discard message if finished.

        // TODO: Pop item from queue.
    }
}

/// Read bytes into a buffer.
///
/// Similar to [`AsyncReadExt::read_buf`], except it performs multiple read calls until at least
/// `target` bytes have been read.
///
/// Will automatically retry if an [`io::ErrorKind::Interrupted`] is returned.
///
/// # Cancellation safety
///
/// This function is cancellation safe in the same way that [`AsyncReadExt::read_buf`] is.
async fn read_atleast_bytesmut<'a, R>(
    reader: &'a mut R,
    buf: &mut BytesMut,
    target: usize,
) -> io::Result<usize>
where
    R: AsyncReadExt + Sized + Unpin,
{
    let mut bytes_read = 0;
    buf.reserve(target);

    while bytes_read < target {
        match reader.read_buf(buf).await {
            Ok(n) => bytes_read += n,
            Err(err) => {
                if matches!(err.kind(), io::ErrorKind::Interrupted) {
                    continue;
                }
                return Err(err);
            }
        }
    }

    Ok(bytes_read)
}
