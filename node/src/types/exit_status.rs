use datasize::DataSize;

/// Exit codes which should be used by the casper-node binary, and provided by the initializer
/// reactor to the binary.
///
/// Note that a panic will result in the Rust process producing an exit code of 101.
#[derive(Clone, Copy, PartialEq, Eq, Debug, DataSize)]
#[repr(u8)]
pub enum ExitStatus {
    /// The process should exit with success.  The launcher should proceed to run the next
    /// installed version of `casper-node`.
    Success = 0,
    /// The process should exit with `101`, equivalent to panicking.  The launcher should not
    /// restart the node.
    Abort = 101,
    /// The process should exit with `102`.  The launcher should proceed to run the previous
    /// installed version of `casper-node`.
    DowngradeVersion = 102,
}
