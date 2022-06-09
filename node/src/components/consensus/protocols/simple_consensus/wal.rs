use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Seek, Write},
    marker::PhantomData,
    mem,
    path::PathBuf,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

use crate::components::consensus::{
    protocols::simple_consensus::message::{Content, Proposal, SignedMessage},
    traits::Context,
};

use super::RoundId;

/// An entry in the Write-Ahead Log, storing a message we had added to our protocol state.
#[derive(Deserialize, Serialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Entry<C: Context> {
    /// A signed echo or vote.
    SignedMessage(SignedMessage<C>),
    /// A proposal.
    Proposal(Proposal<C>, RoundId),
    /// Evidence of a validator double-signing.
    Evidence(SignedMessage<C>, Content<C>, C::Signature),
}

/// A Write-Ahead Log to store every message on disk when we add it to the protocol state.
#[derive(Debug)]
pub(crate) struct WriteWal<C: Context> {
    writer: BufWriter<File>,
    phantom_context: PhantomData<C>,
}

impl<C: Context> DataSize for WriteWal<C> {
    const IS_DYNAMIC: bool = true;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        self.writer.capacity()
    }
}

#[derive(Error, Debug)]
pub(crate) enum WriteWalError {
    #[error("Could not get serialized message size: {0}")]
    CouldntGetSerializedSize(bincode::Error),
    #[error("Could not serialize size: {0}")]
    CouldntSerializeSizeIntoWriter(io::Error),
    #[error("Could not serialize message: {0}")]
    CouldntSerializeMessageIntoWriter(bincode::Error),
    #[error("Could not flush message to disk: {0}")]
    CouldntFlushMessageToDisk(io::Error),
    #[error("Could not open file: {0}")]
    FileCouldntBeOpened(io::Error),
}

impl<C: Context> WriteWal<C> {
    pub(crate) fn new(wal_path: &PathBuf) -> Result<Self, WriteWalError> {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&wal_path)
            .map_err(WriteWalError::FileCouldntBeOpened)?;
        Ok(WriteWal {
            writer: BufWriter::new(file),
            phantom_context: PhantomData,
        })
    }

    pub(crate) fn record_entry(&mut self, entry: &Entry<C>) -> Result<(), WriteWalError> {
        // First write the size of the entry as a serialized u64.
        let entry_size =
            bincode::serialized_size(entry).map_err(WriteWalError::CouldntGetSerializedSize)?;
        self.writer
            .write_all(&entry_size.to_le_bytes())
            .map_err(WriteWalError::CouldntSerializeSizeIntoWriter)?;
        // Write the serialized entry itself.
        bincode::serialize_into(&mut self.writer, entry)
            .map_err(WriteWalError::CouldntSerializeMessageIntoWriter)?;
        self.writer
            .flush()
            .map_err(WriteWalError::CouldntFlushMessageToDisk)?;
        Ok(())
    }
}

/// A buffer to read a Write-Ahead Log from disk and deserialize its messages.
#[derive(Debug)]
pub(crate) struct ReadWal<C: Context> {
    pub(crate) reader: BufReader<File>,
    pub(crate) phantom_context: PhantomData<C>,
}

#[derive(Error, Debug)]
pub(crate) enum ReadWalError {
    #[error("Could not create file at {0}: {1}")]
    FileCouldntBeCreated(PathBuf, io::Error),
    #[error(transparent)]
    OtherIOError(#[from] io::Error),
    #[error("could not deserialize WAL entry: {0}")]
    CouldNotDeserialize(bincode::Error),
}

impl<C: Context> ReadWal<C> {
    pub(crate) fn new(wal_path: &PathBuf) -> Result<Self, ReadWalError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&wal_path)
            .map_err(|err| ReadWalError::FileCouldntBeCreated(wal_path.clone(), err))?;
        Ok(ReadWal {
            reader: BufReader::new(file),
            phantom_context: PhantomData,
        })
    }
}

impl<C: Context> ReadWal<C> {
    /// Reads the next entry from the WAL, or returns an error.
    /// If there are 0 bytes left it returns `Ok(None)`.
    pub(crate) fn read_next_entry(&mut self) -> Result<Option<Entry<C>>, ReadWalError> {
        // Remember the current position: If we encounter an unreadable entry we trim the file at
        // this point so we can continue appending entries after it.
        let position = self.reader.stream_position()?;

        // Deserialize the size of the entry, in bytes, as a u64.
        let mut entry_size_buf = [0u8; mem::size_of::<u64>()];
        if let Err(err) = self.reader.read_exact(&mut entry_size_buf) {
            if err.kind() == io::ErrorKind::UnexpectedEof {
                self.trim_file(position)?;
                return Ok(None);
            }
            return Err(ReadWalError::OtherIOError(err));
        }
        let entry_size = u64::from_le_bytes(entry_size_buf) as usize;

        // Read the serialized entry itself.
        let mut entry_buf = vec![0; entry_size];
        if let Err(err) = self.reader.read_exact(&mut entry_buf) {
            if err.kind() == io::ErrorKind::UnexpectedEof {
                self.trim_file(position)?;
                return Ok(None);
            }
            return Err(ReadWalError::OtherIOError(err));
        }

        // Deserialize and return the entry.
        let entry = bincode::deserialize(&entry_buf).map_err(ReadWalError::CouldNotDeserialize)?;
        Ok(Some(entry))
    }

    /// Trims the file to the given length and logs a warning if any bytes were removed.
    ///
    /// This should be called with the position where the last complete entry ended. Incomplete
    /// entries can safely be removed because we only send messages after writing them and
    /// flushing the buffer, so we won't remove any messages that we already sent.
    fn trim_file(&mut self, position: u64) -> Result<(), ReadWalError> {
        if self.reader.stream_position()? > position {
            warn!("removing incomplete entry from WAL");
            self.reader.get_mut().set_len(position)?;
        }
        Ok(())
    }
}
