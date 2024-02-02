use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    DeployApproval, TransactionV1Approval,
};

/// A struct containing a signature of a transaction hash and the public key of the signer.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum TransactionApproval {
    /// A deploy.
    Deploy(DeployApproval),
    /// A version 1 transaction.
    #[cfg_attr(any(feature = "std", test), serde(rename = "Version1"))]
    V1(TransactionV1Approval),
}

impl From<&DeployApproval> for TransactionApproval {
    fn from(value: &DeployApproval) -> Self {
        Self::Deploy(value.clone())
    }
}

impl From<&TransactionV1Approval> for TransactionApproval {
    fn from(value: &TransactionV1Approval) -> Self {
        Self::V1(value.clone())
    }
}

// TODO[RC]: Add roundtrip test

const DEPLOY_TAG: u8 = 0;
const V1_TAG: u8 = 1;

impl ToBytes for TransactionApproval {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionApproval::Deploy(deploy_approval) => {
                DEPLOY_TAG.write_bytes(writer)?;
                deploy_approval.write_bytes(writer)
            }
            TransactionApproval::V1(v1_approval) => {
                V1_TAG.write_bytes(writer)?;
                v1_approval.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TransactionApproval::Deploy(deploy_approval) => deploy_approval.serialized_length(),
                TransactionApproval::V1(v1_approval) => v1_approval.serialized_length(),
            }
    }
}

impl FromBytes for TransactionApproval {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = FromBytes::from_bytes(bytes)?;
        match tag {
            DEPLOY_TAG => {
                let (deploy_approval, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((TransactionApproval::Deploy(deploy_approval), remainder))
            }
            V1_TAG => {
                let (v1_approval, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((TransactionApproval::V1(v1_approval), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}
