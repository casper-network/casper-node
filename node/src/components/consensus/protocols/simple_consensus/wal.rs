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
    serialize = "C::Hash: Serialize, C::ValidatorId: Serialize",
    deserialize = "C::Hash: Deserialize<'de>, C::ValidatorId: Deserialize<'de>",
))]
pub(super) enum Entry<C: Context> {
    SignedMessage(SignedMessage<C>),
    Proposal(Proposal<C>, RoundId),
    Evidence(SignedMessage<C>, C::ValidatorId, Content<C>, C::Signature),
}

/// The messages are written to disk like:
#[derive(Debug)]
pub(super) struct WriteWAL<C: Context> {
    writer: BufWriter<File>,
    phantom_context: PhantomData<C>,
}

impl<C: Context> DataSize for WriteWAL<C> {
    const IS_DYNAMIC: bool = true;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        self.writer.capacity()
    }
}

/// TODO These cases must be expanded or shrunk based on a case analysis of the
/// underlying causes for failure, right now they're more of an enumeration of
/// the places where errors take place
#[derive(Debug)]
pub(super) enum WriteWALError {
    PathDoesntExist,
    CouldntGetSerializedSize,
    CouldntSerializeSizeIntoWriter,
    CouldntSerializeMessageIntoWriter,
    FileCouldntOpen,
    PathIsntFile,
    CouldntFlushMessageToDisk,
}

impl<C: Context> WriteWAL<C>
where
    C::ValidatorId: Serialize,
{
    pub(super) fn new(wal_path: &PathBuf) -> Result<Self, WriteWALError> {
        let meta = std::fs::metadata(&wal_path).map_err(|_| WriteWALError::PathDoesntExist)?;
        if meta.is_file() {
            let file = OpenOptions::new()
                .append(true)
                .open(&wal_path)
                .map_err(|_| WriteWALError::FileCouldntOpen)?;
            let writer = BufWriter::new(file);
            Ok(WriteWAL {
                writer,
                phantom_context: PhantomData,
            })
        } else {
            Err(WriteWALError::PathIsntFile)
        }
    }

    pub(super) fn record_entry(&mut self, entry: &Entry<C>) -> Result<(), WriteWALError> {
        let entry_size =
            bincode::serialized_size(entry).map_err(|_| WriteWALError::CouldntGetSerializedSize)?;
        self.writer
            .write_all(&entry_size.to_be_bytes())
            .map_err(|_| WriteWALError::CouldntSerializeSizeIntoWriter)?;
        bincode::serialize_into(&mut self.writer, entry)
            .map_err(|_| WriteWALError::CouldntSerializeMessageIntoWriter)?;
        self.writer
            .flush()
            .map_err(|_| WriteWALError::CouldntFlushMessageToDisk)?;
        Ok(())
    }
}

// TODO add a unit variant for the case where the file doesn't exist
#[derive(Debug)]
pub(super) struct ReadWAL<C: Context> {
    pub(super) bytes_left: usize,
    pub(super) reader: BufReader<File>,
    pub(super) phantom_context: PhantomData<C>,
}

/// TODO These cases must be expanded or shrunk based on a case analysis of the
/// underlying causes for failure, right now they're more of an enumeration of
/// the places where errors take place
#[derive(Debug)]
pub(super) enum ReadWALError {
    FileCouldntOpen(PathBuf),
    PathIsntFile(PathBuf),
    NotEnoughBytesForEntry,
    CouldntReadMessageSize,
    CouldntReadMessage,
    CouldntDeserializeMessage,
    NoMoreEntries,
    FileCouldntBeCreated(PathBuf, std::io::Error),
    CouldntGetMeta,
    OtherIOError(std::io::Error),
    CouldntCreateParentDirectory(PathBuf, std::io::Error),
}

impl<C: Context> ReadWAL<C>
where
    C::ValidatorId: for<'de> Deserialize<'de>,
{
    pub(super) fn new(wal_path: &PathBuf) -> Result<Self, ReadWALError> {
        let (file, bytes_left) = match std::fs::metadata(&wal_path) {
            Ok(meta) => {
                if meta.is_file() {
                    let file = OpenOptions::new()
                        .read(true)
                        .open(&wal_path)
                        .map_err(|_| ReadWALError::FileCouldntOpen(wal_path.clone()))?;
                    (file, meta.size() as usize)
                } else {
                    return Err(ReadWALError::PathIsntFile(wal_path.clone()));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent_directory) = wal_path.parent() {
                    std::fs::create_dir_all(parent_directory).map_err(|err| {
                        ReadWALError::CouldntCreateParentDirectory(parent_directory.to_owned(), err)
                    })?;
                }

                let file = OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .append(true)
                    .open(&wal_path)
                    .map_err(|err| ReadWALError::FileCouldntBeCreated(wal_path.clone(), err))?;
                let meta =
                    std::fs::metadata(&wal_path).map_err(|_| ReadWALError::CouldntGetMeta)?;
                (file, meta.size() as usize)
            }
            Err(err) => {
                return Err(ReadWALError::OtherIOError(err));
            }
        };
        let reader = BufReader::new(file);
        Ok(ReadWAL {
            bytes_left,
            reader,
            phantom_context: PhantomData,
        })
    }
}

impl<C: Context> ReadWAL<C>
where
    C::ValidatorId: for<'de> Deserialize<'de>,
{
    pub(super) fn read_next_entry(&mut self) -> Result<Entry<C>, ReadWALError> {
        if self.bytes_left == 0 {
            return Err(ReadWALError::NoMoreEntries);
        }
        let entry_size = {
            let mut entry_size_buf = [0u8; 8];
            self.reader
                .read_exact(&mut entry_size_buf)
                .map_err(|_| ReadWALError::CouldntReadMessageSize)?;
            self.bytes_left -= 8;
            std::primitive::u64::from_be_bytes(entry_size_buf)
        } as usize;
        if entry_size < self.bytes_left {
            return Err(ReadWALError::NotEnoughBytesForEntry);
        }
        let entry = {
            let mut entry_buf = vec![0; entry_size];
            self.reader
                .read_exact(&mut entry_buf)
                .map_err(|_| ReadWALError::CouldntReadMessage)?;
            self.bytes_left -= entry_size;
            bincode::deserialize(&entry_buf).map_err(|_| ReadWALError::CouldntDeserializeMessage)?
        };
        Ok(entry)
    }
}

// TODO Write a test ensuring that the WAL encoding works
#[test]
fn test_roundtrip_wals() {}
