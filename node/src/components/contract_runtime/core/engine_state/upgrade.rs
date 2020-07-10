use std::fmt;

use crate::components::contract_runtime::shared::wasm_costs::WasmCosts;
use crate::components::contract_runtime::shared::{newtypes::Blake2bHash, TypeMismatch};
use crate::components::contract_runtime::storage::global_state::CommitResult;
use types::{bytesrepr, Key, ProtocolVersion};

use crate::components::contract_runtime::core::engine_state::execution_effect::ExecutionEffect;

pub type ActivationPoint = u64;

#[derive(Debug)]
pub enum UpgradeResult {
    RootNotFound,
    KeyNotFound(Key),
    TypeMismatch(TypeMismatch),
    Serialization(bytesrepr::Error),
    Success {
        post_state_hash: Blake2bHash,
        effect: ExecutionEffect,
    },
}

impl fmt::Display for UpgradeResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            UpgradeResult::RootNotFound => write!(f, "Root not found"),
            UpgradeResult::KeyNotFound(key) => write!(f, "Key not found: {}", key),
            UpgradeResult::TypeMismatch(type_mismatch) => {
                write!(f, "Type mismatch: {:?}", type_mismatch)
            }
            UpgradeResult::Serialization(error) => write!(f, "Serialization error: {:?}", error),
            UpgradeResult::Success {
                post_state_hash,
                effect,
            } => write!(f, "Success: {} {:?}", post_state_hash, effect),
        }
    }
}

impl UpgradeResult {
    pub fn from_commit_result(commit_result: CommitResult, effect: ExecutionEffect) -> Self {
        match commit_result {
            CommitResult::RootNotFound => UpgradeResult::RootNotFound,
            CommitResult::KeyNotFound(key) => UpgradeResult::KeyNotFound(key),
            CommitResult::TypeMismatch(type_mismatch) => UpgradeResult::TypeMismatch(type_mismatch),
            CommitResult::Serialization(error) => UpgradeResult::Serialization(error),
            CommitResult::Success { state_root, .. } => UpgradeResult::Success {
                post_state_hash: state_root,
                effect,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeConfig {
    pre_state_hash: Blake2bHash,
    current_protocol_version: ProtocolVersion,
    new_protocol_version: ProtocolVersion,
    upgrade_installer_args: Option<Vec<u8>>,
    upgrade_installer_bytes: Option<Vec<u8>>,
    wasm_costs: Option<WasmCosts>,
    activation_point: Option<ActivationPoint>,
}

impl UpgradeConfig {
    pub fn new(
        pre_state_hash: Blake2bHash,
        current_protocol_version: ProtocolVersion,
        new_protocol_version: ProtocolVersion,
        upgrade_installer_args: Option<Vec<u8>>,
        upgrade_installer_bytes: Option<Vec<u8>>,
        wasm_costs: Option<WasmCosts>,
        activation_point: Option<ActivationPoint>,
    ) -> Self {
        UpgradeConfig {
            pre_state_hash,
            current_protocol_version,
            new_protocol_version,
            upgrade_installer_args,
            upgrade_installer_bytes,
            wasm_costs,
            activation_point,
        }
    }

    pub fn pre_state_hash(&self) -> Blake2bHash {
        self.pre_state_hash
    }

    pub fn current_protocol_version(&self) -> ProtocolVersion {
        self.current_protocol_version
    }

    pub fn new_protocol_version(&self) -> ProtocolVersion {
        self.new_protocol_version
    }

    pub fn upgrade_installer_args(&self) -> Option<&[u8]> {
        let args = self.upgrade_installer_args.as_ref()?;
        Some(args.as_slice())
    }

    pub fn upgrade_installer_bytes(&self) -> Option<&[u8]> {
        let bytes = self.upgrade_installer_bytes.as_ref()?;
        Some(bytes.as_slice())
    }

    pub fn wasm_costs(&self) -> Option<WasmCosts> {
        self.wasm_costs
    }

    pub fn activation_point(&self) -> Option<u64> {
        self.activation_point
    }
}
