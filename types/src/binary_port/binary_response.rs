//! The binary response.

use core::any;

use tracing::error;

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHash, BlockHashAndHeight, PeersMap,
};

use super::{binary_request::BinaryRequest, db_id::DbId, DbRawBytesSpec, PROTOCOL_VERSION};

#[repr(u8)]
pub enum PayloadType {
    BlockHeaderV1,
    BlockHeader,
    BlockBodyV1,
    BlockBody,
    ApprovalsHashes,
    ApprovalsHashesV1, // TODO[RC]: not existing yet
    BlockSignatures,
    Deploy,
    Transaction,
    ExecutionResultV1,
    ExecutionResult,
    VecTransfers,
    VecU8,
    FinalizedDeployApprovals,
    FinalizedApprovals,
    Bool,
    BlockHashAndHeight,
    BlockHash,
    PeersMap,
}

impl PayloadType {
    pub(crate) fn new_from_db_id(db_id: &DbId, is_legacy: bool) -> Self {
        match (is_legacy, db_id) {
            (true, DbId::BlockHeader) => Self::BlockHeaderV1,
            (true, DbId::BlockBody) => Self::BlockBodyV1,
            (true, DbId::ApprovalsHashes) => Self::ApprovalsHashes,
            (true, DbId::BlockMetadata) => Self::BlockSignatures,
            (true, DbId::Transaction) => Self::Deploy,
            (true, DbId::ExecutionResult) => Self::ExecutionResultV1,
            (true, DbId::Transfer) => Self::VecTransfers,
            (true, DbId::StateStore) => Self::VecU8,
            (true, DbId::FinalizedTransactionApprovals) => Self::FinalizedDeployApprovals,
            (false, DbId::BlockHeader) => Self::BlockHeader,
            (false, DbId::BlockBody) => Self::BlockBody,
            (false, DbId::ApprovalsHashes) => Self::ApprovalsHashesV1,
            (false, DbId::BlockMetadata) => Self::BlockSignatures,
            (false, DbId::Transaction) => Self::Transaction,
            (false, DbId::ExecutionResult) => Self::ExecutionResult,
            (false, DbId::Transfer) => Self::VecTransfers,
            (false, DbId::StateStore) => Self::VecU8,
            (false, DbId::FinalizedTransactionApprovals) => Self::FinalizedApprovals,
        }
    }
}

impl From<bool> for PayloadType {
    fn from(_: bool) -> Self {
        Self::Bool
    }
}

impl<T> From<Option<T>> for PayloadType {
    fn from(_: Option<T>) -> Self {
        panic!("could this be a compile time error?");
    }
}

impl From<BlockHashAndHeight> for PayloadType {
    fn from(_: BlockHashAndHeight) -> Self {
        Self::BlockHashAndHeight
    }
}

impl From<BlockHash> for PayloadType {
    fn from(_: BlockHash) -> Self {
        Self::BlockHash
    }
}

impl From<PeersMap> for PayloadType {
    fn from(_: PeersMap) -> Self {
        Self::PeersMap
    }
}

const BLOCK_HEADER_V1_TAG: u8 = 0;
const BLOCK_HEADER_TAG: u8 = 1;
const BLOCK_BODY_V1_TAG: u8 = 2;
const BLOCK_BODY_TAG: u8 = 3;
const APPROVALS_HASHES_TAG: u8 = 4;
const APPROVALS_HASHES_V1: u8 = 5;
const BLOCK_SIGNATURES_TAG: u8 = 6;
const DEPLOY_TAG: u8 = 7;
const TRANSACTION_TAG: u8 = 8;
const EXECUTION_RESULT_V1_TAG: u8 = 9;
const EXECUTION_RESULT_TAG: u8 = 10;
const VEC_TRANSFERS_TAG: u8 = 11;
const VEC_U8_TAG: u8 = 12;
const FINALIZED_DEPLOY_APPROVALS_TAG: u8 = 13;
const FINALIZED_APPROVALS_TAG: u8 = 14;
const BOOL_TAG: u8 = 15;
const BLOCK_HASH_AND_HEIGHT_TAG: u8 = 16;
const BLOCK_HASH_TAG: u8 = 17;
const PEERS_MAP_TAG: u8 = 18;

impl ToBytes for PayloadType {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PayloadType::BlockHeaderV1 => BLOCK_HEADER_V1_TAG,
            PayloadType::BlockHeader => BLOCK_HEADER_TAG,
            PayloadType::BlockBodyV1 => BLOCK_BODY_V1_TAG,
            PayloadType::BlockBody => BLOCK_BODY_TAG,
            PayloadType::ApprovalsHashes => APPROVALS_HASHES_TAG,
            PayloadType::ApprovalsHashesV1 => APPROVALS_HASHES_V1,
            PayloadType::BlockSignatures => BLOCK_SIGNATURES_TAG,
            PayloadType::Deploy => DEPLOY_TAG,
            PayloadType::Transaction => TRANSACTION_TAG,
            PayloadType::ExecutionResultV1 => EXECUTION_RESULT_V1_TAG,
            PayloadType::ExecutionResult => EXECUTION_RESULT_TAG,
            PayloadType::VecTransfers => VEC_TRANSFERS_TAG,
            PayloadType::VecU8 => VEC_U8_TAG,
            PayloadType::FinalizedDeployApprovals => FINALIZED_DEPLOY_APPROVALS_TAG,
            PayloadType::FinalizedApprovals => FINALIZED_APPROVALS_TAG,
            PayloadType::Bool => BOOL_TAG,
            PayloadType::BlockHashAndHeight => BLOCK_HASH_AND_HEIGHT_TAG,
            PayloadType::BlockHash => BLOCK_HASH_TAG,
            PayloadType::PeersMap => PEERS_MAP_TAG,
        }
        .write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for PayloadType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = FromBytes::from_bytes(bytes)?;
        let db_id = match tag {
            BLOCK_HEADER_V1_TAG => PayloadType::BlockHeaderV1,
            BLOCK_HEADER_TAG => PayloadType::BlockHeader,
            BLOCK_BODY_V1_TAG => PayloadType::BlockBodyV1,
            BLOCK_BODY_TAG => PayloadType::BlockBody,
            APPROVALS_HASHES_TAG => PayloadType::ApprovalsHashes,
            APPROVALS_HASHES_V1 => PayloadType::ApprovalsHashesV1,
            BLOCK_SIGNATURES_TAG => PayloadType::BlockSignatures,
            DEPLOY_TAG => PayloadType::Deploy,
            TRANSACTION_TAG => PayloadType::Transaction,
            EXECUTION_RESULT_V1_TAG => PayloadType::ExecutionResultV1,
            EXECUTION_RESULT_TAG => PayloadType::ExecutionResult,
            VEC_TRANSFERS_TAG => PayloadType::VecTransfers,
            VEC_U8_TAG => PayloadType::VecU8,
            FINALIZED_DEPLOY_APPROVALS_TAG => PayloadType::FinalizedDeployApprovals,
            FINALIZED_APPROVALS_TAG => PayloadType::FinalizedApprovals,
            BOOL_TAG => PayloadType::Bool,
            BLOCK_HASH_AND_HEIGHT_TAG => PayloadType::BlockHashAndHeight,
            BLOCK_HASH_TAG => PayloadType::BlockHash,
            PEERS_MAP_TAG => PayloadType::PeersMap,
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok((db_id, remainder))
    }
}

pub struct BinaryResponseHeader {
    /// single byte - binary protocol version
    protocol_version: u8,
    /// single byte - return value - 0-ok, 1..255 error
    /// if not 0, no more bytes will follow
    error: u8,
    /// returned data type - u8 repr enum
    returned_data_type: Option<PayloadType>,
}

impl BinaryResponseHeader {
    pub fn new(returned_data_type: Option<PayloadType>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            error: 0,
            returned_data_type,
        }
    }

    pub fn returned_data_type(&self) -> Option<&PayloadType> {
        self.returned_data_type.as_ref()
    }
}

impl ToBytes for BinaryResponseHeader {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let Self {
            protocol_version,
            error,
            returned_data_type,
        } = self;

        protocol_version.write_bytes(writer)?;
        error.write_bytes(writer)?;
        returned_data_type.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.protocol_version.serialized_length()
            + self.error.serialized_length()
            + self.returned_data_type.serialized_length()
    }
}

impl FromBytes for BinaryResponseHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (protocol_version, remainder) = FromBytes::from_bytes(bytes)?;
        let (error, remainder) = FromBytes::from_bytes(remainder)?;
        let (payload_type, remainder) = FromBytes::from_bytes(remainder)?;

        Ok((
            BinaryResponseHeader {
                protocol_version,
                error,
                returned_data_type: payload_type,
            },
            remainder,
        ))
    }
}

pub struct BinaryResponse {
    pub header: BinaryResponseHeader,
    /// bytesrepr serialized original binary request
    pub original_request: Vec<u8>,
    /// payload
    pub payload: Vec<u8>,
}

// We'll never be returning Ok(None), it'll always be Ok(Some(...)) pushed through Juliet

impl BinaryResponse {
    pub fn new_error() -> Self {
        todo!()
    }

    pub fn from_db_raw_bytes(
        db_id: &DbId,
        binary_request: BinaryRequest,
        spec: Option<DbRawBytesSpec>,
    ) -> Self {
        match spec {
            Some(DbRawBytesSpec {
                is_legacy,
                raw_bytes,
            }) => BinaryResponse {
                header: BinaryResponseHeader::new(Some(PayloadType::new_from_db_id(
                    db_id, is_legacy,
                ))),
                original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
                payload: raw_bytes,
            },
            None => BinaryResponse {
                header: BinaryResponseHeader::new(None),
                original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
                payload: vec![],
            },
        }
    }

    // TODO[RC]: Can we prevent V from being an Option here?
    pub fn from_value<V>(binary_request: BinaryRequest, val: V) -> Self
    where
        V: ToBytes,
        V: Into<PayloadType>,
    {
        BinaryResponse {
            payload: ToBytes::to_bytes(&val).unwrap(),
            header: BinaryResponseHeader::new(Some(val.into())),
            original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
        }
    }

    pub fn from_opt<V>(binary_request: BinaryRequest, val: Option<V>) -> Self
    where
        V: ToBytes,
        V: Into<PayloadType>,
    {
        match val {
            Some(val) => Self::from_value(binary_request, val),
            None => BinaryResponse {
                payload: vec![],
                header: BinaryResponseHeader::new(None),
                original_request: ToBytes::to_bytes(&binary_request).unwrap(), // TODO[RC]: Do not serialize here, thread the original serialized request into here
            },
        }
    }
}

impl ToBytes for BinaryResponse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let BinaryResponse {
            header,
            original_request,
            payload,
        } = self;

        header.write_bytes(writer)?;
        original_request.write_bytes(writer)?;
        payload.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.header.serialized_length()
            + self.original_request.serialized_length()
            + self.payload.serialized_length()
    }
}

impl FromBytes for BinaryResponse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (header, remainder) = FromBytes::from_bytes(bytes)?;
        let (original_request, remainder) = Bytes::from_bytes(remainder)?;
        let (payload, remainder) = Bytes::from_bytes(remainder)?;

        Ok((
            BinaryResponse {
                header,
                original_request: original_request.into(),
                payload: payload.into(),
            },
            remainder,
        ))
    }
}
