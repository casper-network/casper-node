//! Pidfile utility type.
//!
//! Pidfiles are used to gate access to a resource, as well as detect unclean shutdowns.

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

/// A PID file.
///
/// Records the current process' PID, removes it on exit. Can be used to determine whether or not
/// an application was shut down cleanly.
///
/// The pidfile is held open with an exclusive but advisory lock.
#[derive(Debug)]
pub struct Pidfile {
    /// The pidfile.
    ///
    /// The file will be locked for the lifetime of `Pidfile`.
    pidfile: File,
    /// The pidfile location.
    path: PathBuf,
    /// Previous pidfile contents.
    previous: Option<u32>,
}

/// An error acquiring a pidfile.
#[derive(Debug, Error)]
pub enum PidfileError {
    /// The pidfile could not be opened at all.
    #[error("could not pidfile: {0}")]
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
    /// We encountere a previous PID, but it was the same as ours.
    ///
    /// This should never happen, as the exclusive lock should prevent it.
    #[error("read back our own pid from exlusively locked pidfile")]
    DuplicatedPid,
}

/// Pidfile outcome.
///
/// High-level description of the outcome of opening and locking the PIDfile.
#[must_use]
#[derive(Debug)]
pub enum PidfileOutcome {
    /// Another instance of the node is likely running, or an attempt was made to reuse a pidfile.
    ///
    /// **Recommendation**: Exit to avoid resource conflicts.
    AnotherNodeRunning(PidfileError),
    /// The node crashed previously and could potentially have been corrupted.
    ///
    /// **Recommendation**: Run an integrity check, then potentially continue with intialization.
    ///                     **Store the `Pidfile`**.
    Crashed(Pidfile),
    /// Clean start, pidfile lock acquired.
    ///
    /// **Recommendation**: Continue with intialization, but **store the `Pidfile`**.
    Clean(Pidfile),
    /// There was an error managing the pidfile, not sure if we have crashed or not.
    ///
    /// **Recommendation**: Exit, as it will not be possible to determine a crash at the next
    /// start.
    PidfileError(PidfileError),
}

impl Pidfile {
    /// Acquire a `Pidfile` and give an actionable outcome.
    ///
    /// **Important**: This function should be called **before** opening whatever resources it is
    /// protecting.
    pub fn acquire<P: AsRef<Path>>(path: P) -> PidfileOutcome {
        match Pidfile::new(path) {
            Ok(pidfile) => {
                if pidfile.unclean_shutdown() {
                    PidfileOutcome::Crashed(pidfile)
                } else {
                    PidfileOutcome::Clean(pidfile)
                }
            }
            Err(err @ PidfileError::LockFailed(_)) => PidfileOutcome::AnotherNodeRunning(err),
            Err(err) => PidfileOutcome::PidfileError(err),
        }
    }

    /// Creates a new pidfile.
    ///
    /// The error-behavior of this function is important and can be used to distinguish between
    /// different conditions according to the table below. If the `Pidfile` is instantiated before
    /// the resource it is supposed to protect, the following actions are recommended:
    fn new<P: AsRef<Path>>(path: P) -> Result<Pidfile, PidfileError> {
        // First we try to open the pidfile, without disturbing it.
        let mut pidfile = fs::OpenOptions::new()
            .truncate(false)
            .create(true)
            .read(true)
            .write(true)
            .open(path.as_ref())
            .map_err(PidfileError::CouldNotOpen)?;

        // Now try to acquire an exclusive lock. This will fail if another process or another
        // instance of `Pidfile` is holding a lock onto the same pidfile.
        pidfile
            .try_lock_exclusive()
            .map_err(PidfileError::LockFailed)?;

        // At this point, we're the exclusive users of the file and can read its contents.
        let mut raw_contents = String::new();
        pidfile
            .read_to_string(&mut raw_contents)
            .map_err(PidfileError::ReadFailed)?;

        // Note: We cannot distinguish an empty file from a non-existing file, unfortunately.
        let previous = if raw_contents == "" {
            None
        } else {
            Some(raw_contents.parse().map_err(PidfileError::Corrupted)?)
        };

        let pid = process::id();

        // If we encounter our own PID, we got extremely unlucky, or something went really wrong.
        if previous == Some(pid) {
            return Err(PidfileError::DuplicatedPid);
        }

        // Truncate and rewind.
        pidfile.set_len(0).map_err(PidfileError::WriteFailed)?;
        pidfile
            .seek(SeekFrom::Start(0))
            .map_err(PidfileError::WriteFailed)?;

        // Do our best to ensure that we always have some contents in the file immediately.
        pidfile
            .write_all(pid.to_string().as_bytes())
            .map_err(PidfileError::WriteFailed)?;

        pidfile.flush().map_err(PidfileError::WriteFailed)?;

        Ok(Pidfile {
            pidfile,
            path: path.as_ref().to_owned(),
            previous,
        })
    }

    /// Whether or not the Pidfile indicated a previously unclean shutdown.
    fn unclean_shutdown(&self) -> bool {
        // If there are any previous contents, we crashed. We check for our own PID already before.
        self.previous.is_some()
    }
}

impl Drop for Pidfile {
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

    use super::{Pidfile, PidfileOutcome};

    #[test]
    fn pidfile_creates_file_and_cleans_it_up() {
        let tmp_dir = TempDir::new().expect("could not create tmp_dir");
        let pidfile_path = tmp_dir.path().join("create_and_cleanup.pid");

        let outcome = Pidfile::acquire(&pidfile_path);

        match outcome {
            PidfileOutcome::Clean(pidfile) => {
                // Check the pidfile exists, the verify it gets removed after dropping the pidfile.
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

        let outcome = Pidfile::acquire(&pidfile_path);

        match outcome {
            PidfileOutcome::Crashed(pidfile) => {
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

        let outcome = Pidfile::acquire(&pidfile_path);

        match outcome {
            PidfileOutcome::Clean(_pidfile) => {
                match Pidfile::acquire(&pidfile_path) {
                    PidfileOutcome::AnotherNodeRunning(_) => {
                        // All good, lets ensure the loaded pid matches our current pid.
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
