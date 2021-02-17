use std::fmt::{Display, Formatter};

use serde::Serialize;

use casper_client::Error as ClientError;

/// This struct defines possible errors specific to this particular RPC request.
#[derive(Serialize)]
pub enum Error {
    /// The Block requested does not exist.
    UnknownBlock(String),
    /// The deploy requested does not exist.
    NoSuchDeploy(String),
    /// The deploy sent to the node is invalid.
    InvalidDeploy(String),
    /// An unknown error was encountered.
    UnknownError(String),
}

impl From<ClientError> for Error {
    fn from(error: ClientError) -> Self {
        if let ClientError::ResponseIsError(rpc_error) = error {
            match rpc_error.code {
                -32001 => Error::UnknownBlock(rpc_error.message),
                -32000 => Error::NoSuchDeploy(rpc_error.message),
                -32008 => Error::InvalidDeploy(rpc_error.message),
                _ => Error::UnknownError(rpc_error.message),
            }
        } else {
            panic!("Unknown error encountered: {:?}", error)
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnknownBlock(message) => write!(f, "Could not get requested block: {}", message),
            Error::NoSuchDeploy(message) => {
                write!(f, "The requested deploy was not found: {}", message)
            }
            Error::InvalidDeploy(message) => write!(f, "The deploy sent is invalid: {}", message),
            Error::UnknownError(message) => {
                write!(f, "An unknown error was encountered: {}", message)
            }
        }
    }
}
