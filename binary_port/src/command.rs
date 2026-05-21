use core::convert::TryFrom;

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    Transaction,
};

use crate::get_request::GetRequest;

#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;

/// The header of a binary request.
#[derive(Debug, PartialEq)]
pub struct CommandHeader {
    header_version: u16,
    type_tag: u8,
    id: u16,
}

impl CommandHeader {
    // Defines the current version of the header, in practice defining the current version of the
    // binary port protocol. Requests with mismatched header version will be dropped.
    pub const HEADER_VERSION: u16 = 1;

    /// Creates new binary request header.
    pub fn new(type_tag: CommandTag, id: u16) -> Self {
        Self {
            header_version: Self::HEADER_VERSION,
            type_tag: type_tag.into(),
            id,
        }
    }

    /// Returns the type tag of the request.
    pub fn type_tag(&self) -> u8 {
        self.type_tag
    }

    /// Returns the request id.
    pub fn id(&self) -> u16 {
        self.id
    }

    /// Returns the header version.
    pub fn version(&self) -> u16 {
        self.header_version
    }

    #[cfg(any(feature = "testing", test))]
    pub fn set_binary_request_version(&mut self, version: u16) {
        self.header_version = version;
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            header_version: rng.gen(),
            type_tag: CommandTag::random(rng).into(),
            id: rng.gen(),
        }
    }
}

impl ToBytes for CommandHeader {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.header_version.write_bytes(writer)?;
        self.type_tag.write_bytes(writer)?;
        self.id.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.header_version.serialized_length()
            + self.type_tag.serialized_length()
            + self.id.serialized_length()
    }
}

impl FromBytes for CommandHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (binary_request_version, remainder) = FromBytes::from_bytes(bytes)?;
        let (type_tag, remainder) = FromBytes::from_bytes(remainder)?;
        let (id, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            CommandHeader {
                header_version: binary_request_version,
                type_tag,
                id,
            },
            remainder,
        ))
    }
}

/// A request to the binary access interface.
#[derive(Debug, PartialEq)]

pub enum Command {
    /// Request to get data from the node
    Get(GetRequest),
    /// Request to add a transaction into a blockchain.
    TryAcceptTransaction {
        /// Transaction to be handled.
        transaction: Transaction,
    },
    /// Request to execute a transaction speculatively.
    TrySpeculativeExec {
        /// Transaction to execute.
        transaction: Transaction,
    },
}

impl Command {
    /// Returns the type tag of the request.
    pub fn tag(&self) -> CommandTag {
        match self {
            Command::Get(_) => CommandTag::Get,
            Command::TryAcceptTransaction { .. } => CommandTag::TryAcceptTransaction,
            Command::TrySpeculativeExec { .. } => CommandTag::TrySpeculativeExec,
        }
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match CommandTag::random(rng) {
            CommandTag::Get => Self::Get(GetRequest::random(rng)),
            CommandTag::TryAcceptTransaction => Self::TryAcceptTransaction {
                transaction: Transaction::random(rng),
            },
            CommandTag::TrySpeculativeExec => Self::TrySpeculativeExec {
                transaction: Transaction::random(rng),
            },
        }
    }
}

impl ToBytes for Command {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            Command::Get(inner) => inner.write_bytes(writer),
            Command::TryAcceptTransaction { transaction } => transaction.write_bytes(writer),
            Command::TrySpeculativeExec { transaction } => transaction.write_bytes(writer),
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            Command::Get(inner) => inner.serialized_length(),
            Command::TryAcceptTransaction { transaction } => transaction.serialized_length(),
            Command::TrySpeculativeExec { transaction } => transaction.serialized_length(),
        }
    }
}

impl TryFrom<(CommandTag, &[u8])> for Command {
    type Error = bytesrepr::Error;

    fn try_from((tag, bytes): (CommandTag, &[u8])) -> Result<Self, Self::Error> {
        let (req, remainder) = match tag {
            CommandTag::Get => {
                let (get_request, remainder) = FromBytes::from_bytes(bytes)?;
                (Command::Get(get_request), remainder)
            }
            CommandTag::TryAcceptTransaction => {
                let (transaction, remainder) = FromBytes::from_bytes(bytes)?;
                (Command::TryAcceptTransaction { transaction }, remainder)
            }
            CommandTag::TrySpeculativeExec => {
                let (transaction, remainder) = FromBytes::from_bytes(bytes)?;
                (Command::TrySpeculativeExec { transaction }, remainder)
            }
        };
        if !remainder.is_empty() {
            return Err(bytesrepr::Error::LeftOverBytes);
        }
        Ok(req)
    }
}

/// The type tag of a binary request.
#[derive(Debug, PartialEq)]
#[repr(u8)]
pub enum CommandTag {
    /// Request to get data from the node
    Get = 0,
    /// Request to add a transaction into a blockchain.
    TryAcceptTransaction = 1,
    /// Request to execute a transaction speculatively.
    TrySpeculativeExec = 2,
}

impl CommandTag {
    /// Creates a random `CommandTag`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => CommandTag::Get,
            1 => CommandTag::TryAcceptTransaction,
            2 => CommandTag::TrySpeculativeExec,
            _ => unreachable!(),
        }
    }
}

impl TryFrom<u8> for CommandTag {
    type Error = InvalidCommandTag;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(CommandTag::Get),
            1 => Ok(CommandTag::TryAcceptTransaction),
            2 => Ok(CommandTag::TrySpeculativeExec),
            _ => Err(InvalidCommandTag),
        }
    }
}

impl From<CommandTag> for u8 {
    fn from(value: CommandTag) -> Self {
        value as u8
    }
}

/// Error raised when trying to convert an invalid u8 into a `CommandTag`.
pub struct InvalidCommandTag;

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn header_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = CommandHeader::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }

    #[test]
    fn request_bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = Command::random(rng);
        let bytes = val.to_bytes().expect("should serialize");
        assert_eq!(Command::try_from((val.tag(), &bytes[..])), Ok(val));
    }
}
