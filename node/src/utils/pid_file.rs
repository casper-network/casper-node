//! PidFile utility type.
//!
//! PidFiles are used to gate access to a resource, as well as detect unclean shutdowns.

use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process,
};

use fs2::FileExt;
use thiserror::Error;
use tracing::warn;

/// A PID (process ID) file.
///
/// Records the current process' PID, removes it on exit. Can be used to determine whether or not
/// an application was shut down cleanly.
///
/// The pid_file is held open with an exclusive but advisory lock.
#[derive(Debug)]
pub(crate) struct PidFile {
    /// The pid_file.
    ///
    /// The file will be locked for the lifetime of `PidFile`.
    _pid_file: File,
    /// The pid_file location.
    path: PathBuf,
    /// Previous pid_file contents.
    previous: PreviousContents,
}

/// An error acquiring a pid_file.
#[derive(Debug, Error)]
pub(crate) enum PidFileError {
    /// The pid_file could not be opened at all.
    #[error("could not open pid_file: {0}")]
    CouldNotOpen(#[source] io::Error),
    /// The pid_file could not be locked.
    #[error("could not lock pid_file: {0}")]
    LockFailed(#[source] io::Error),
    /// Error reading pid_file contents.
    #[error("reading existing pid_file failed: {0}")]
    ReadFailed(#[source] io::Error),
    /// Error writing pid_file contents.
    #[error("updating pid_file failed: {0}")]
    WriteFailed(#[source] io::Error),
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum PreviousContents {
    /// Set if the PidFile's contents can be parsed as a `u32`.
    Pid(u32),
    /// Set if the user provides a non-empty PidFile which doesn't parse as a `u32`.
    NonNumeric,
    /// Set if the PidFile doesn't exist or has no contents.
    None,
}

/// PidFile outcome.
///
/// High-level description of the outcome of opening and locking the PIDfile.
#[must_use]
#[derive(Debug)]
pub(crate) enum PidFileOutcome {
    /// Another instance of the node is likely running, or an attempt was made to reuse a pid_file.
    ///
    /// **Recommendation**: Exit to avoid resource conflicts.
    AnotherNodeRunning(PidFileError),
    /// The node crashed previously and could potentially have been corrupted.
    ///
    /// **Recommendation**: Run an integrity check, then potentially continue with initialization.
    ///                     **Store the `PidFile`**.
    Crashed(PidFile),
    /// The user manually created an invalid PidFile to force integrity checks.
    ForceIntegrityChecks(PidFile),
    /// Clean start, pid_file lock acquired.
    ///
    /// **Recommendation**: Continue with initialization, but **store the `PidFile`**.
    Clean(PidFile),
    /// There was an error managing the PidFile, not sure if we have crashed or not.
    ///
    /// **Recommendation**: Exit, as it will not be possible to determine a crash at the next
    /// start.
    PidFileError(PidFileError),
}

impl PidFile {
    /// Acquire a `PidFile` and give an actionable outcome.
    ///
    /// **Important**: This function should be called **before** opening whatever resources it is
    /// protecting.
    pub(crate) fn acquire<P: AsRef<Path>>(path: P) -> PidFileOutcome {
        match PidFile::new(path) {
            Ok(pid_file) => match pid_file.previous {
                PreviousContents::Pid(_) => PidFileOutcome::Crashed(pid_file),
                PreviousContents::NonNumeric => PidFileOutcome::ForceIntegrityChecks(pid_file),
                PreviousContents::None => PidFileOutcome::Clean(pid_file),
            },
            Err(err @ PidFileError::LockFailed(_)) => PidFileOutcome::AnotherNodeRunning(err),
            Err(err) => PidFileOutcome::PidFileError(err),
        }
    }

    /// Creates a new pid_file.
    ///
    /// The error-behavior of this function is important and can be used to distinguish between
    /// different conditions described in [`PidFileError`]. If the `PidFile` is instantiated before
    /// the resource it is supposed to protect, the following actions are recommended:
    fn new<P: AsRef<Path>>(path: P) -> Result<PidFile, PidFileError> {
        // First we try to open the pid_file, without disturbing it.
        let mut pid_file = fs::OpenOptions::new()
            .truncate(false)
            .create(true)
            .read(true)
            .write(true)
            .open(path.as_ref())
            .map_err(PidFileError::CouldNotOpen)?;

        // Now try to acquire an exclusive lock. This will fail if another process or another
        // instance of `PidFile` is holding a lock onto the same pid_file.
        pid_file
            .try_lock_exclusive()
            .map_err(PidFileError::LockFailed)?;

        // At this point, we're the exclusive users of the file and can read its contents.
        let mut raw_contents = String::new();
        pid_file
            .read_to_string(&mut raw_contents)
            .map_err(PidFileError::ReadFailed)?;

        // Note: We cannot distinguish an empty file from a non-existing file, unfortunately.
        let previous = if raw_contents.is_empty() {
            PreviousContents::None
        } else {
            match raw_contents.parse() {
                Ok(previous_pid) => PreviousContents::Pid(previous_pid),
                Err(_) => PreviousContents::NonNumeric,
            }
        };

        let pid = process::id();

        // Truncate and rewind.
        pid_file.set_len(0).map_err(PidFileError::WriteFailed)?;
        pid_file
            .seek(SeekFrom::Start(0))
            .map_err(PidFileError::WriteFailed)?;

        // Do our best to ensure that we always have some contents in the file immediately.
        pid_file
            .write_all(pid.to_string().as_bytes())
            .map_err(PidFileError::WriteFailed)?;

        pid_file.flush().map_err(PidFileError::WriteFailed)?;

        Ok(PidFile {
            _pid_file: pid_file,
            path: path.as_ref().to_owned(),
            previous,
        })
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        // When dropping the pid_file, we delete its file. We are still keeping the logs and the
        // opened file handle, which will get cleaned up naturally.
        if let Err(err) = fs::remove_file(&self.path) {
            warn!(path=%self.path.display(), %err, "could not delete pid_file");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn pid_file_creates_file_and_cleans_it_up() {
        let tmp_dir = TempDir::new().expect("could not create tmp_dir");
        let pid_file_path = tmp_dir.path().join("create_and_cleanup.pid");

        let outcome = PidFile::acquire(&pid_file_path);

        match outcome {
            PidFileOutcome::Clean(pid_file) => {
                // Check the pid_file exists, then verify it gets removed after dropping the
                // pid_file.
                assert!(pid_file_path.exists());
                drop(pid_file);
                assert!(!pid_file_path.exists());
            }
            other => panic!("pid_file outcome not clean, but {:?}", other),
        }
    }

    #[test]
    fn detects_crash() {
        let tmp_dir = TempDir::new().expect("could not create tmp_dir");
        let pid_file_path = tmp_dir.path().join("create_and_cleanup.pid");

        // We create a garbage pid_file to simulate an unclean shutdown.
        fs::write(&pid_file_path, b"12345").expect("could not write garbage pid file");

        let outcome = PidFile::acquire(&pid_file_path);

        match outcome {
            PidFileOutcome::Crashed(pid_file) => {
                // Now check if the written pid matches our PID.
                assert_eq!(pid_file.previous, PreviousContents::Pid(12345));

                // After we've crashed, we still expect cleanup.
                assert!(pid_file_path.exists());
                drop(pid_file);
                assert!(!pid_file_path.exists());
            }
            other => panic!("pid_file outcome did not detect crash, is {:?}", other),
        }
    }

    #[test]
    fn detects_user_forced_pidfile() {
        let tmp_dir = TempDir::new().expect("could not create tmp_dir");
        let pid_file_path = tmp_dir.path().join("create_and_cleanup.pid");

        // We create a garbage pid_file to simulate an unclean shutdown.
        fs::write(&pid_file_path, b"not an integer").expect("could not write garbage pid file");

        let outcome = PidFile::acquire(&pid_file_path);

        match outcome {
            PidFileOutcome::ForceIntegrityChecks(pid_file) => {
                // Now check if the written pid matches our PID.
                assert_eq!(pid_file.previous, PreviousContents::NonNumeric);

                // After we've crashed, we still expect cleanup.
                assert!(pid_file_path.exists());
                drop(pid_file);
                assert!(!pid_file_path.exists());
            }
            other => panic!("pid_file outcome did not detect crash, is {:?}", other),
        }
    }

    #[test]
    fn blocks_second_instance() {
        let tmp_dir = TempDir::new().expect("could not create tmp_dir");
        let pid_file_path = tmp_dir.path().join("create_and_cleanup.pid");

        let outcome = PidFile::acquire(&pid_file_path);

        match outcome {
            PidFileOutcome::Clean(_pid_file) => {
                match PidFile::acquire(&pid_file_path) {
                    PidFileOutcome::AnotherNodeRunning(_) => {
                        // All good, this is what we expected.
                    }
                    other => panic!(
                        "expected detection of duplicate pid_file access, instead got: {:?}",
                        other
                    ),
                }
            }
            other => panic!("pid_file outcome not clean, but {:?}", other),
        }
    }
}