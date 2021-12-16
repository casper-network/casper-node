use datasize::DataSize;
use signal_hook::consts::signal::{SIGINT, SIGQUIT, SIGTERM};

/// The offset Rust uses by default when generating an exit code after being interrupted by a
/// termination signal.
const SIGNAL_OFFSET: u8 = 128;

/// Exit codes which should be used by the casper-node binary, and provided by the initializer
/// reactor to the binary.
///
/// Note that a panic will result in the Rust process producing an exit code of 101.
#[derive(Clone, Copy, PartialEq, Eq, Debug, DataSize)]
#[repr(u8)]
pub enum ExitCode {
    /// The process should exit with success.  The launcher should proceed to run the next
    /// installed version of `casper-node`.
    Success = 0,
    /// The process should exit with `101`, equivalent to panicking.  The launcher should not
    /// restart the node.
    Abort = 101,
    /// The process should exit with `102`.  It used to be an indication to the launcher
    /// that it should proceed to run the previous installed version of `casper-node`.
    /// It is no longer used, but we keep it here to avoid it being reassigned to other features.
    #[doc(hidden)]
    DowngradeVersion = 102,
    /// The exit code Rust uses by default when interrupted via an `INT` signal.
    SigInt = SIGNAL_OFFSET + SIGINT as u8,
    /// The exit code Rust uses by default when interrupted via a `QUIT` signal.
    SigQuit = SIGNAL_OFFSET + SIGQUIT as u8,
    /// The exit code Rust uses by default when interrupted via a `TERM` signal.
    SigTerm = SIGNAL_OFFSET + SIGTERM as u8,
}
