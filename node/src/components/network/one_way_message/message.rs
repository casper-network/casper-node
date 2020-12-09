use std::{
    fmt::{self, Debug, Display, Formatter},
    future::Future,
    io,
    pin::Pin,
};

use futures::{AsyncReadExt, AsyncWriteExt, FutureExt};
use futures_io::{AsyncRead, AsyncWrite};
use libp2p::request_response::RequestResponseCodec;
use serde::{Deserialize, Serialize};

use super::ProtocolId;
use crate::{components::network::Config, types::NodeId};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Message<P>(P);

impl<P> Message<P> {
    pub fn new(payload: P) -> Self {
        Message(payload)
    }

    pub fn into_payload(self) -> P {
        self.0
    }
}

impl<P: Display> Display for Message<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "payload: {}", self.0)
    }
}

#[derive(Debug)]
pub(in crate::components::network) struct IncomingMessage<P> {
    pub source: NodeId,
    pub message: Message<P>,
}

#[derive(Debug)]
pub(in crate::components::network) struct OutgoingMessage<P> {
    pub destination: NodeId,
    pub message: Message<P>,
}

/// Implements libp2p `RequestResponseCodec` for one-way messages, i.e. requests which expect no
/// response.
#[derive(Debug, Clone)]
pub struct Codec {
    max_message_size: u32,
}

impl From<&Config> for Codec {
    fn from(config: &Config) -> Self {
        Codec {
            max_message_size: config.max_one_way_message_size,
        }
    }
}

impl RequestResponseCodec for Codec {
    type Protocol = ProtocolId;
    type Request = Vec<u8>;
    type Response = ();

    fn read_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 Self::Protocol,
        io: &'life2 mut T,
    ) -> Pin<Box<dyn Future<Output = io::Result<Self::Request>> + 'async_trait + Send>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
        T: AsyncRead + Unpin + Send + 'async_trait,
    {
        async move {
            // Read the length.
            let mut buffer = [0; 4];
            io.read(&mut buffer[..])
                .await
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
            let length = u32::from_le_bytes(buffer);
            if length > self.max_message_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "message size exceeds limit: {} > {}",
                        length, self.max_message_size
                    ),
                ));
            }

            // Read the payload.
            let mut buffer = vec![0; length as usize];
            io.read_exact(&mut buffer).await?;
            Ok(buffer)
        }
        .boxed()
    }

    fn read_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 Self::Protocol,
        _io: &'life2 mut T,
    ) -> Pin<Box<dyn Future<Output = io::Result<Self::Response>> + 'async_trait + Send>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
        T: AsyncRead + Unpin + Send + 'async_trait,
    {
        // For one-way messages, where no response will be sent by the peer, just return Ok(()).
        async { Ok(()) }.boxed()
    }

    fn write_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 Self::Protocol,
        io: &'life2 mut T,
        request: Self::Request,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'async_trait + Send>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
        T: AsyncWrite + Unpin + Send + 'async_trait,
    {
        async move {
            // Write the length.
            if request.len() > self.max_message_size as usize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "message size exceeds limit: {} > {}",
                        request.len(),
                        self.max_message_size
                    ),
                ));
            }
            let length = request.len() as u32;
            io.write_all(&length.to_le_bytes()).await?;

            // Write the payload.
            io.write_all(&request).await?;

            io.close().await?;
            Ok(())
        }
        .boxed()
    }

    fn write_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 Self::Protocol,
        _io: &'life2 mut T,
        _response: Self::Response,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + 'async_trait + Send>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
        T: AsyncWrite + Unpin + Send + 'async_trait,
    {
        // For one-way messages, where no response will be sent by the peer, just return Ok(()).
        async { Ok(()) }.boxed()
    }
}
