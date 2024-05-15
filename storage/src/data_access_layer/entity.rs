use std::collections::BTreeSet;

use serde::Serialize;
use thiserror::Error;
use tracing::error;

use casper_types::{
    account::AccountHash, addressable_entity::Weight, bytesrepr::FromBytes, execution::Effects,
    system::entity, CLTyped, CLValue, CLValueError, Digest, InitiatorAddr, ProtocolVersion,
    RuntimeArgs, TransactionEntryPoint, TransactionHash,
};

use crate::{
    system::runtime_native::Config as NativeRuntimeConfig, tracking_copy::TrackingCopyError,
};

/// An error returned when constructing an [`EntityMethod`].
#[derive(Clone, Eq, PartialEq, Error, Serialize, Debug)]
pub enum EntityMethodError {
    /// Provided entry point is not one of the Entity ones.
    #[error("invalid entry point for entity: {0}")]
    InvalidEntryPoint(TransactionEntryPoint),
    /// Required arg missing.
    #[error("missing '{0}' arg")]
    MissingArg(String),
    /// Failed to parse the given arg.
    #[error("failed to parse '{arg}' arg: {error}")]
    CLValue {
        /// The arg name.
        arg: String,
        /// The failure.
        error: CLValueError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityMethod {
    AddAssociatedKey {
        account: AccountHash,
        weight: Weight,
    },
    UpdateAssociatedKey {
        account: AccountHash,
        weight: Weight,
    },
    RemoveAssociatedKey {
        account: AccountHash,
    },
}

impl EntityMethod {
    pub fn from_parts(
        entry_point: TransactionEntryPoint,
        runtime_args: &RuntimeArgs,
    ) -> Result<Self, EntityMethodError> {
        match entry_point {
            TransactionEntryPoint::AddAssociatedKey => Self::new_add_associated_key(runtime_args),
            TransactionEntryPoint::UpdateAssociatedKey => {
                Self::new_update_associated_key(runtime_args)
            }
            TransactionEntryPoint::RemoveAssociatedKey => {
                Self::new_remove_associated_key(runtime_args)
            }
            _ => Err(EntityMethodError::InvalidEntryPoint(entry_point)),
        }
    }

    fn new_add_associated_key(runtime_args: &RuntimeArgs) -> Result<Self, EntityMethodError> {
        let account = Self::get_named_argument(runtime_args, entity::ARG_ACCOUNT)?;
        let weight = Self::get_named_argument(runtime_args, entity::ARG_WEIGHT)?;
        Ok(Self::AddAssociatedKey { account, weight })
    }

    fn new_update_associated_key(runtime_args: &RuntimeArgs) -> Result<Self, EntityMethodError> {
        let account = Self::get_named_argument(runtime_args, entity::ARG_ACCOUNT)?;
        let weight = Self::get_named_argument(runtime_args, entity::ARG_WEIGHT)?;
        Ok(Self::UpdateAssociatedKey { account, weight })
    }

    fn new_remove_associated_key(runtime_args: &RuntimeArgs) -> Result<Self, EntityMethodError> {
        let account = Self::get_named_argument(runtime_args, entity::ARG_ACCOUNT)?;
        Ok(Self::RemoveAssociatedKey { account })
    }

    fn get_named_argument<T: FromBytes + CLTyped>(
        args: &RuntimeArgs,
        name: &str,
    ) -> Result<T, EntityMethodError> {
        let arg: &CLValue = args
            .get(name)
            .ok_or_else(|| EntityMethodError::MissingArg(name.to_string()))?;
        arg.to_t().map_err(|error| EntityMethodError::CLValue {
            arg: name.to_string(),
            error,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRequest {
    /// The runtime config.
    pub(crate) config: NativeRuntimeConfig,
    /// State root hash.
    pub(crate) state_hash: Digest,
    /// The protocol version.
    pub(crate) protocol_version: ProtocolVersion,
    /// The auction method.
    pub(crate) entity_method: EntityMethod,
    /// Transaction hash.
    pub(crate) transaction_hash: TransactionHash,
    /// Base account.
    pub(crate) initiator: InitiatorAddr,
    /// List of authorizing accounts.
    pub(crate) authorization_keys: BTreeSet<AccountHash>,
}

impl EntityRequest {
    /// Creates new request instance with runtime args.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: NativeRuntimeConfig,
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        initiator: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        entity_method: EntityMethod,
    ) -> Self {
        Self {
            config,
            state_hash,
            protocol_version,
            transaction_hash,
            initiator,
            authorization_keys,
            entity_method,
        }
    }

    pub fn config(&self) -> &NativeRuntimeConfig {
        &self.config
    }

    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub fn entity_method(&self) -> &EntityMethod {
        &self.entity_method
    }

    pub fn transaction_hash(&self) -> TransactionHash {
        self.transaction_hash
    }

    pub fn initiator(&self) -> &InitiatorAddr {
        &self.initiator
    }

    pub fn authorization_keys(&self) -> &BTreeSet<AccountHash> {
        &self.authorization_keys
    }
}

#[derive(Debug)]
pub enum EntityResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Bidding request succeeded
    Success {
        /// Effects of bidding interaction.
        effects: Effects,
    },
    /// Bidding request failed.
    Failure(TrackingCopyError),
}

impl EntityResult {
    /// Is this a success.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Effects.
    pub fn effects(&self) -> Effects {
        match self {
            Self::RootNotFound | Self::Failure(_) => Effects::new(),
            Self::Success { effects } => effects.clone(),
        }
    }
}
