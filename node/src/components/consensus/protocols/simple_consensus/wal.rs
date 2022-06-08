use std::{
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    marker::PhantomData,
    os::unix::fs::MetadataExt,
    path::PathBuf,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

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

#[derive(Debug)]
pub(crate) enum WriteWalError {
    CouldntGetSerializedSize,
    CouldntSerializeSizeIntoWriter,
    CouldntSerializeMessageIntoWriter,
    FileCouldntOpen,
    PathIsntFile,
    CouldntFlushMessageToDisk,
    FileCouldntBeCreated(std::io::Error),
    CouldntCreateParentDirectory(PathBuf, std::io::Error),
    OtherIOError(std::io::Error),
}

impl<C: Context> WriteWal<C> {
    pub(crate) fn new(wal_path: &PathBuf) -> Result<Self, WriteWalError> {
        let file = match std::fs::metadata(&wal_path) {
            Ok(meta) => {
                if meta.is_file() {
                    let file = OpenOptions::new()
                        .append(true)
                        .open(&wal_path)
                        .map_err(|_| WriteWalError::FileCouldntOpen)?;
                    Ok(file)
                } else {
                    Err(WriteWalError::PathIsntFile)
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent_directory) = wal_path.parent() {
                    std::fs::create_dir_all(parent_directory).map_err(|err| {
                        WriteWalError::CouldntCreateParentDirectory(
                            parent_directory.to_owned(),
                            err,
                        )
                    })?;
                }

                OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&wal_path)
                    .map_err(WriteWalError::FileCouldntBeCreated)
            }
            Err(err) => Err(WriteWalError::OtherIOError(err)),
        }?;

        let writer = BufWriter::new(file);

        Ok(WriteWal {
            writer,
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
    pub(crate) bytes_left: usize,
    pub(crate) total_bytes: usize,
    pub(crate) reader: BufReader<File>,
    pub(crate) phantom_context: PhantomData<C>,
}

#[derive(Debug)]
pub(crate) enum ReadWalError {
    FileCouldntOpen(PathBuf),
    PathIsntFile(PathBuf),
    CouldntReadMessage,
    NoMoreEntries,
    FileCouldntBeCreated(PathBuf, std::io::Error),
    CouldntGetMeta,
    OtherIOError(std::io::Error),
    CouldntCreateParentDirectory(PathBuf, std::io::Error),
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
        let (file, bytes_left) = match std::fs::metadata(&wal_path) {
            Ok(meta) => {
                if meta.is_file() {
                    let file = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&wal_path)
                        .map_err(|_| ReadWalError::FileCouldntOpen(wal_path.clone()))?;
                    (file, meta.size() as usize)
                } else {
                    return Err(ReadWalError::PathIsntFile(wal_path.clone()));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent_directory) = wal_path.parent() {
                    std::fs::create_dir_all(parent_directory).map_err(|err| {
                        ReadWalError::CouldntCreateParentDirectory(parent_directory.to_owned(), err)
                    })?;
                }

                let file = OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .open(&wal_path)
                    .map_err(|err| ReadWalError::FileCouldntBeCreated(wal_path.clone(), err))?;
                let meta =
                    std::fs::metadata(&wal_path).map_err(|_| ReadWalError::CouldntGetMeta)?;
                (file, meta.size() as usize)
            }
            Err(err) => {
                return Err(ReadWalError::OtherIOError(err));
            }
        };
        let reader = BufReader::new(file);
        Ok(ReadWal {
            bytes_left,
            total_bytes: bytes_left,
            reader,
            phantom_context: PhantomData,
        })
    }
}

impl<C: Context> ReadWal<C> {
    #[allow(clippy::integer_arithmetic)] // TODO
    pub(crate) fn read_next_entry(&mut self) -> Result<Entry<C>, ReadWalError> {
        if self.bytes_left == 0 {
            return Err(ReadWalError::NoMoreEntries);
        }
        if self.bytes_left < 8 {
            self.reader
                .get_mut()
                .set_len((self.total_bytes - self.bytes_left) as u64)
                .map_err(ReadWalError::OtherIOError)?;
            return Err(ReadWalError::EndedOnCorruption(
                WalCorruptionType::NotEnoughInput,
            ));
        }
        let mut entry_size_buf = [0u8; 8];
        match self.reader.read_exact(&mut entry_size_buf) {
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                // TODO Should we keep this? Mostly keeping it here as a sanity check for now.
                unreachable!()
            }
            Err(err) => {
                return Err(ReadWalError::OtherIOError(err));
            }
            Ok(()) => {}
        }
        self.bytes_left -= 8;
        let entry_size = std::primitive::u64::from_be_bytes(entry_size_buf) as usize;
        if entry_size > self.bytes_left {
            self.reader
                .get_mut()
                .set_len((self.total_bytes - self.bytes_left - 8) as u64)
                .map_err(ReadWalError::OtherIOError)?;
            return Err(ReadWalError::EndedOnCorruption(
                WalCorruptionType::NotEnoughInput,
            ));
        }
        let entry = {
            let mut entry_buf = vec![0; entry_size];
            self.reader
                .read_exact(&mut entry_buf)
                .map_err(|_| ReadWalError::CouldntReadMessage)?;
            self.bytes_left -= entry_size;
            bincode::deserialize(&entry_buf).map_err(|_| {
                ReadWalError::EndedOnCorruption(WalCorruptionType::ImproperFormatting)
            })?
        };
        Ok(entry)
    }
}

// TODO Write a test ensuring that the WAL encoding works
#[test]
fn test_roundtrip_wals() {}
