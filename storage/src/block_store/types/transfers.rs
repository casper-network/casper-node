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
pub(in crate::block_store) struct Transfers(Vec<Transfer>);

impl Transfers {
    pub(in crate::block_store) fn into_owned(self) -> Vec<Transfer> {
        self.0
    }
}

impl From<Vec<TransferV1>> for Transfers {
    fn from(v1_transfers: Vec<TransferV1>) -> Self {
        Transfers(v1_transfers.into_iter().map(Transfer::V1).collect())
    }
}

impl From<Vec<Transfer>> for Transfers {
    fn from(transfers: Vec<Transfer>) -> Self {
        Transfers(transfers)
    }
}

impl ToBytes for Transfers {
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

impl FromBytes for Transfers {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Vec::<Transfer>::from_bytes(bytes)
            .map(|(transfers, remainder)| (Transfers(transfers), remainder))
    }
}
