//! Code execution.
mod error;
#[macro_use]
mod executor;

pub use self::error::Error as ExecError;
pub(crate) use self::executor::Executor;
