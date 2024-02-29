use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    Transfer, TransferV1,
};

/// A wrapped `Vec<Transfer>`, used as the value type in the `transfer_dbs`.
///
/// It exists to allow the `impl From<Vec<TransferV1>>` to be written, making the type suitable for
/// use as a parameter in a `VersionedDatabases`.
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
pub(in crate::block_store) struct Transfers<'a>(Cow<'a, Vec<Transfer>>);

impl<'a> Transfers<'a> {
    pub(in crate::block_store) fn into_owned(self) -> Vec<Transfer> {
        self.0.into_owned()
    }
}

impl<'a> From<Vec<TransferV1>> for Transfers<'a> {
    fn from(v1_transfers: Vec<TransferV1>) -> Self {
        Transfers(Cow::Owned(
            v1_transfers.into_iter().map(Transfer::V1).collect(),
        ))
    }
}

impl<'a> From<Vec<Transfer>> for Transfers<'a> {
    fn from(transfers: Vec<Transfer>) -> Self {
        Transfers(Cow::Owned(transfers))
    }
}

impl<'a> From<&'a Vec<Transfer>> for Transfers<'a> {
    fn from(transfers: &'a Vec<Transfer>) -> Self {
        Transfers(Cow::Borrowed(transfers))
    }
}

impl<'a> ToBytes for Transfers<'a> {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }
}

impl<'a> FromBytes for Transfers<'a> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Vec::<Transfer>::from_bytes(bytes)
            .map(|(transfers, remainder)| (Transfers(Cow::Owned(transfers)), remainder))
    }
}
