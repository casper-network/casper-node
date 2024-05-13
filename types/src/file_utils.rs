//! Utilities for handling reading from and writing to files.

use std::{
    fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use thiserror::Error;

/// Error reading a file.
#[derive(Debug, Error)]
#[error("could not read '{0}': {error}", .path.display())]
pub struct ReadFileError {
    /// Path that failed to be read.
    path: PathBuf,
    /// The underlying OS error.
    #[source]
    error: io::Error,
}

/// Error writing a file
#[derive(Debug, Error)]
#[error("could not write to '{0}': {error}", .path.display())]
pub struct WriteFileError {
    /// Path that failed to be written to.
    path: PathBuf,
    /// The underlying OS error.
    #[source]
    error: io::Error,
}

/// Read complete at `path` into memory.
///
/// Wraps `fs::read`, but preserves the filename for better error printing.
pub fn read_file<P: AsRef<Path>>(filename: P) -> Result<Vec<u8>, ReadFileError> {
    let path = filename.as_ref();
    fs::read(path).map_err(|error| ReadFileError {
        path: path.to_owned(),
        error,
    })
}

/// Write data to `path`.
///
/// Wraps `fs::write`, but preserves the filename for better error printing.
pub(crate) fn write_file<P: AsRef<Path>, B: AsRef<[u8]>>(
    filename: P,
    data: B,
) -> Result<(), WriteFileError> {
    let path = filename.as_ref();
    fs::write(path, data.as_ref()).map_err(|error| WriteFileError {
        path: path.to_owned(),
        error,
    })
}

/// Writes data to `path`, ensuring only the owner can read or write it.
///
/// Otherwise functions like [`write_file`].
pub(crate) fn write_private_file<P: AsRef<Path>, B: AsRef<[u8]>>(
    filename: P,
    data: B,
) -> Result<(), WriteFileError> {
    let path = filename.as_ref();
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(data.as_ref()))
        .map_err(|error| WriteFileError {
            path: path.to_owned(),
            error,
        })
}
