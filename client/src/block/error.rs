use std::fmt::{Display, Formatter};

use serde::Serialize;

use casper_client::Error as ClientError;

/// An enum that holds the possible errors for this specific RPC request.
#[derive(Serialize)]
pub enum Error {
    /// The block that was requested by the RPC is unknown or does not exist.
    UnknownBlock(String),
    /// An unknown error.
    UnknownError(String),
}

impl From<ClientError> for Error {
    fn from(error: ClientError) -> Self {
        if let ClientError::ResponseIsError(error_message) = error {
            match error_message.code {
                -32001 => Error::UnknownBlock(error_message.message),
                _ => Error::UnknownError(error_message.message),
            }
        } else {
            panic!("Failed due to: {:?}", error)
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnknownError(message) => {
                write!(f, "An unknown error was encountered: {}", message)
            }
            Error::UnknownBlock(message) => write!(f, "Could not get requested block: {}", message),
        }
    }
}