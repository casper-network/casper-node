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

use crate::components::consensus::{
    protocols::simple_consensus::message::{Content, Proposal, SignedMessage},
    traits::Context,
};

use super::RoundId;

#[derive(Deserialize, Serialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Entry<C: Context> {
    SignedMessage(SignedMessage<C>),
    Proposal(Proposal<C>, RoundId),
    Evidence(SignedMessage<C>, Content<C>, C::Signature),
}

/// The messages are written to disk like:
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
    #[error("Could not get serialized message size")]
    CouldntGetSerializedSize,
    #[error("Could not serialize size")]
    CouldntSerializeSizeIntoWriter,
    #[error("Could not serialize message")]
    CouldntSerializeMessageIntoWriter,
    #[error("Could not flush message to disk")]
    CouldntFlushMessageToDisk,
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
        let entry_size =
            bincode::serialized_size(entry).map_err(|_| WriteWalError::CouldntGetSerializedSize)?;
        self.writer
            .write_all(&entry_size.to_be_bytes())
            .map_err(|_| WriteWalError::CouldntSerializeSizeIntoWriter)?;
        bincode::serialize_into(&mut self.writer, entry)
            .map_err(|_| WriteWalError::CouldntSerializeMessageIntoWriter)?;
        self.writer
            .flush()
            .map_err(|_| WriteWalError::CouldntFlushMessageToDisk)?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct ReadWal<C: Context> {
    pub(crate) reader: BufReader<File>,
    pub(crate) phantom_context: PhantomData<C>,
}

#[derive(Error, Debug)]
pub(crate) enum ReadWalError {
    #[error("No more entries; reached end of file")]
    NoMoreEntries,
    #[error("Could not create file at {0}: {1}")]
    FileCouldntBeCreated(PathBuf, io::Error),
    #[error(transparent)]
    OtherIOError(#[from] io::Error),
    #[error("WAL file corruption: {:?}", .0)]
    EndedOnCorruption(WalCorruptionType),
}

#[derive(Debug)]
pub(crate) enum WalCorruptionType {
    /// Normal case of corruption, likely caused by abruptly shutting the node down.
    NotEnoughInput,
    /// Abnormal case of corruption, likely caused by operator error.
    ImproperFormatting,
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
    pub(crate) fn read_next_entry(&mut self) -> Result<Entry<C>, ReadWalError> {
        let position = self.reader.stream_position()?;

        let mut entry_size_buf = [0u8; mem::size_of::<u64>()];
        if let Err(err) = self.reader.read_exact(&mut entry_size_buf) {
            if err.kind() != io::ErrorKind::UnexpectedEof {
                return Err(ReadWalError::OtherIOError(err));
            }
        } else {
            let entry_size = std::primitive::u64::from_be_bytes(entry_size_buf) as usize;
            let mut entry_buf = vec![0; entry_size];
            if let Err(err) = self.reader.read_exact(&mut entry_buf) {
                if err.kind() != io::ErrorKind::UnexpectedEof {
                    return Err(ReadWalError::OtherIOError(err));
                }
            } else {
                return bincode::deserialize(&entry_buf).map_err(|_| {
                    ReadWalError::EndedOnCorruption(WalCorruptionType::ImproperFormatting)
                });
            }
        }

        if self.reader.stream_position()? == position {
            return Err(ReadWalError::NoMoreEntries);
        }
        self.reader.get_mut().set_len(position)?;
        Err(ReadWalError::EndedOnCorruption(
            WalCorruptionType::NotEnoughInput,
        ))
    }
}

// TODO Write a test ensuring that the WAL encoding works
#[test]
fn test_roundtrip_wals() {}
