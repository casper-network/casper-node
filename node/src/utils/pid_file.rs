//! PidFile utility type.
//!
//! PidFiles are used to gate access to a resource, as well as detect unclean shutdowns.

use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    num::ParseIntError,
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
/// The pidfile is held open with an exclusive but advisory lock.
#[derive(Debug)]
pub(crate) struct PidFile {
    /// The pidfile.
    ///
    /// The file will be locked for the lifetime of `PidFile`.
    _pidfile: File,
    /// The pidfile location.
    path: PathBuf,
    /// Previous pidfile contents.
    previous: Option<u32>,
}

/// An error acquiring a pidfile.
#[derive(Debug, Error)]
pub(crate) enum PidFileError {
    /// The pidfile could not be opened at all.
    #[error("could not open pidfile: {0}")]
    CouldNotOpen(#[source] io::Error),
    /// The pidfile could not be locked.
    #[error("could not lock pidfile: {0}")]
    LockFailed(#[source] io::Error),
    /// Error reading pidfile contents.
    #[error("reading existing pidfile failed: {0}")]
    ReadFailed(#[source] io::Error),
    /// Error writing pidfile contents.
    #[error("updating pidfile failed: {0}")]
    WriteFailed(#[source] io::Error),
    /// The pidfile was corrupted, its contents could not be read.
    #[error("corrupt pidfile")]
    Corrupted(ParseIntError),
}

/// PidFile outcome.
///
/// High-level description of the outcome of opening and locking the PIDfile.
#[must_use]
#[derive(Debug)]
pub(crate) enum PidFileOutcome {
    /// Another instance of the node is likely running, or an attempt was made to reuse a pidfile.
    ///
    /// **Recommendation**: Exit to avoid resource conflicts.
    AnotherNodeRunning(PidFileError),
    /// The node crashed previously and could potentially have been corrupted.
    ///
    /// **Recommendation**: Run an integrity check, then potentially continue with initialization.
    ///                     **Store the `PidFile`**.
    Crashed(PidFile),
    /// Clean start, pidfile lock acquired.
    ///
    /// **Recommendation**: Continue with initialization, but **store the `PidFile`**.
    Clean(PidFile),
    /// There was an error managing the pidfile, not sure if we have crashed or not.
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
            Ok(pidfile) => {
                if pidfile.unclean_shutdown() {
                    PidFileOutcome::Crashed(pidfile)
                } else {
                    PidFileOutcome::Clean(pidfile)
                }
            }
            Err(err @ PidFileError::LockFailed(_)) => PidFileOutcome::AnotherNodeRunning(err),
            Err(err) => PidFileOutcome::PidFileError(err),
        }
    }

    /// Creates a new pidfile.
    ///
    /// The error-behavior of this function is important and can be used to distinguish between
    /// different conditions described in [`PidFileError`]. If the `PidFile` is instantiated before
    /// the resource it is supposed to protect, the following actions are recommended:
    fn new<P: AsRef<Path>>(path: P) -> Result<PidFile, PidFileError> {
        // First we try to open the pidfile, without disturbing it.
        let mut pidfile = fs::OpenOptions::new()
            .truncate(false)
            .create(true)
            .read(true)
            .write(true)
            .open(path.as_ref())
            .map_err(PidFileError::CouldNotOpen)?;

        // Now try to acquire an exclusive lock. This will fail if another process or another
        // instance of `PidFile` is holding a lock onto the same pidfile.
        pidfile
            .try_lock_exclusive()
            .map_err(PidFileError::LockFailed)?;

        // At this point, we're the exclusive users of the file and can read its contents.
        let mut raw_contents = String::new();
        pidfile
            .read_to_string(&mut raw_contents)
            .map_err(PidFileError::ReadFailed)?;

        // Note: We cannot distinguish an empty file from a non-existing file, unfortunately.
        let previous = if raw_contents.is_empty() {
            None
        } else {
            Some(raw_contents.parse().map_err(PidFileError::Corrupted)?)
        };

        let pid = process::id();

        // Truncate and rewind.
        pidfile.set_len(0).map_err(PidFileError::WriteFailed)?;
        pidfile
            .seek(SeekFrom::Start(0))
            .map_err(PidFileError::WriteFailed)?;

        // Do our best to ensure that we always have some contents in the file immediately.
        pidfile
            .write_all(pid.to_string().as_bytes())
            .map_err(PidFileError::WriteFailed)?;

        pidfile.flush().map_err(PidFileError::WriteFailed)?;

        Ok(PidFile {
            _pidfile: pidfile,
            path: path.as_ref().to_owned(),
            previous,
        })
    }

    /// Whether or not the PidFile indicated a previously unclean shutdown.
    fn unclean_shutdown(&self) -> bool {
        // If there are any previous contents, we crashed. We check for our own PID already before.
        self.previous.is_some()
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        // When dropping the pidfile, we delete its file. We are still keeping the logs and the
        // opened file handle, which will get cleaned up naturally.
        if let Err(err) = fs::remove_file(&self.path) {
            warn!(path=%self.path.display(), %err, "could not delete pidfile");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{PidFile, PidFileOutcome};

    #[test]
    fn pidfile_creates_file_and_cleans_it_up() {
        let tmp_dir = TempDir::new().expect("could not create tmp_dir");
        let pidfile_path = tmp_dir.path().join("create_and_cleanup.pid");

        let outcome = PidFile::acquire(&pidfile_path);

        match outcome {
            PidFileOutcome::Clean(pidfile) => {
                // Check the pidfile exists, then verify it gets removed after dropping the pidfile.
                assert!(pidfile_path.exists());
                drop(pidfile);
                assert!(!pidfile_path.exists());
            }
            other => panic!("pidfile outcome not clean, but {:?}", other),
        }
    }

    #[test]
    fn detects_unclean_shutdown() {
        let tmp_dir = TempDir::new().expect("could not create tmp_dir");
        let pidfile_path = tmp_dir.path().join("create_and_cleanup.pid");

        // We create a garbage pidfile to simulate an unclean shutdown.
        fs::write(&pidfile_path, b"12345").expect("could not write garbage pid file");

        let outcome = PidFile::acquire(&pidfile_path);

        match outcome {
            PidFileOutcome::Crashed(pidfile) => {
                // Now check if the written pid matches our PID.
                assert_eq!(pidfile.previous, Some(12345));

                // After we've crashed, we still expect cleanup.
                assert!(pidfile_path.exists());
                drop(pidfile);
                assert!(!pidfile_path.exists());
            }
            other => panic!("pidfile outcome did not detect crash, is {:?}", other),
        }
    }

    #[test]
    fn blocks_second_instance() {
        let tmp_dir = TempDir::new().expect("could not create tmp_dir");
        let pidfile_path = tmp_dir.path().join("create_and_cleanup.pid");

        let outcome = PidFile::acquire(&pidfile_path);

        match outcome {
            PidFileOutcome::Clean(_pidfile) => {
                match PidFile::acquire(&pidfile_path) {
                    PidFileOutcome::AnotherNodeRunning(_) => {
                        // All good, this is what we expected.
                    }
                    other => panic!(
                        "expected detection of duplicate pidfile access, instead got: {:?}",
                        other
                    ),
                }
            }
            other => panic!("pidfile outcome not clean, but {:?}", other),
        }
    }
}
