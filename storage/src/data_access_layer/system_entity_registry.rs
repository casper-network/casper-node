use crate::tracking_copy::TrackingCopyError;
use casper_types::{
    system::{AUCTION, HANDLE_PAYMENT, MINT},
    Digest, Key, ProtocolVersion, SystemEntityRegistry,
};

/// Used to specify is the requestor wants the registry itself or a named entry within it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemEntityRegistrySelector {
    All,
    ByName(String),
}

impl SystemEntityRegistrySelector {
    /// Create instance asking for the entire registry.
    pub fn all() -> Self {
        SystemEntityRegistrySelector::All
    }

    /// Create instance asking for mint.
    pub fn mint() -> Self {
        SystemEntityRegistrySelector::ByName(MINT.to_string())
    }

    /// Create instance asking for auction.
    pub fn auction() -> Self {
        SystemEntityRegistrySelector::ByName(AUCTION.to_string())
    }

    /// Create instance asking for handle payment.
    pub fn handle_payment() -> Self {
        SystemEntityRegistrySelector::ByName(HANDLE_PAYMENT.to_string())
    }

    /// Name of selected entity, if any.
    pub fn name(&self) -> Option<String> {
        match self {
            SystemEntityRegistrySelector::All => None,
            SystemEntityRegistrySelector::ByName(name) => Some(name.clone()),
        }
    }
}

/// Represents a request to obtain the system entity registry or an entry within it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemEntityRegistryRequest {
    /// State root hash.
    state_hash: Digest,
    /// Protocol version.
    protocol_version: ProtocolVersion,
    /// Selector.
    selector: SystemEntityRegistrySelector,
}

impl SystemEntityRegistryRequest {
    /// Create new request.
    pub fn new(
        state_hash: Digest,
        protocol_version: ProtocolVersion,
        selector: SystemEntityRegistrySelector,
    ) -> Self {
        SystemEntityRegistryRequest {
            state_hash,
            protocol_version,
            selector,
        }
    }

    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// The selector.
    pub fn selector(&self) -> &SystemEntityRegistrySelector {
        &self.selector
    }

    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}

/// The payload of a successful request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemEntityRegistryPayload {
    All(SystemEntityRegistry),
    EntityKey(Key),
}

/// The result of a system entity registry request.
#[derive(Debug)]
pub enum SystemEntityRegistryResult {
    /// Invalid state root hash.
    RootNotFound,
    /// The system contract registry was not found. This is a valid outcome
    /// on older networks, which did not have the system contract registry prior
    /// to protocol version 1.4
    SystemEntityRegistryNotFound,
    /// The named entity was not found in the registry.
    NamedEntityNotFound(String),
    /// Successful request.
    Success {
        /// What was asked for.
        selected: SystemEntityRegistrySelector,
        /// The payload asked for.
        payload: SystemEntityRegistryPayload,
    },
    /// Failed to get requested data.
    Failure(TrackingCopyError),
}

impl SystemEntityRegistryResult {
    pub fn is_success(&self) -> bool {
        matches!(self, SystemEntityRegistryResult::Success { .. })
    }

    pub fn as_legacy(&self) -> Result<SystemEntityRegistryPayload, String> {
        match self {
            SystemEntityRegistryResult::RootNotFound => Err("Root not found".to_string()),
            SystemEntityRegistryResult::SystemEntityRegistryNotFound => {
                Err("System entity registry not found".to_string())
            }
            SystemEntityRegistryResult::NamedEntityNotFound(name) => {
                Err(format!("Named entity not found: {:?}", name))
            }
            SystemEntityRegistryResult::Failure(tce) => Err(format!("{:?}", tce)),
            SystemEntityRegistryResult::Success { payload, .. } => Ok(payload.clone()),
        }
    }
}
