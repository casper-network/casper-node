//! Support for a genesis process.
#![allow(unused_imports)]

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fmt, iter,
    rc::Rc,
};

use itertools::Itertools;
use num::Zero;
use num_rational::Ratio;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use casper_types::{
    addressable_entity::{
        ActionThresholds, EntityKind, EntityKindTag, MessageTopics, NamedKeyAddr, NamedKeyValue,
    },
    bytesrepr,
    contracts::NamedKeys,
    execution::Effects,
    system::{
        auction::{
            self, BidAddr, BidKind, DelegationRate, Delegator, SeigniorageRecipientV2,
            SeigniorageRecipients, SeigniorageRecipientsSnapshot, SeigniorageRecipientsSnapshotV2,
            SeigniorageRecipientsV2, Staking, ValidatorBid, AUCTION_DELAY_KEY,
            DEFAULT_SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION, DELEGATION_RATE_DENOMINATOR,
            ERA_END_TIMESTAMP_MILLIS_KEY, ERA_ID_KEY, INITIAL_ERA_END_TIMESTAMP_MILLIS,
            INITIAL_ERA_ID, LOCKED_FUNDS_PERIOD_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY,
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_VERSION_KEY, UNBONDING_DELAY_KEY, VALIDATOR_SLOTS_KEY,
        },
        handle_payment::{self, ACCUMULATION_PURSE_KEY},
        mint::{
            self, ARG_ROUND_SEIGNIORAGE_RATE, MINT_GAS_HOLD_HANDLING_KEY,
            MINT_GAS_HOLD_INTERVAL_KEY, ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY,
        },
        SystemEntityType, AUCTION, HANDLE_PAYMENT, MINT,
    },
    AccessRights, AddressableEntity, AddressableEntityHash, AdministratorAccount, BlockGlobalAddr,
    BlockTime, ByteCode, ByteCodeAddr, ByteCodeHash, ByteCodeKind, CLValue, Chainspec,
    ChainspecRegistry, Digest, EntityAddr, EntityVersions, EntryPointAddr, EntryPointValue,
    EntryPoints, EraId, FeeHandling, GenesisAccount, GenesisConfig, Groups, HashAddr, Key, Motes,
    Package, PackageHash, PackageStatus, Phase, ProtocolVersion, PublicKey, RefundHandling,
    StoredValue, SystemConfig, SystemHashRegistry, Tagged, TimeDiff, URef, WasmConfig, U512,
};

use crate::{
    global_state::state::StateProvider,
    system::genesis::{
        account_contract_installer::AccountContractInstaller,
        entity_installer::EntityGenesisInstaller,
    },
    tracking_copy::{TrackingCopy, TrackingCopyError},
    AddressGenerator,
};

mod account_contract_installer;
mod entity_installer;

const DEFAULT_ADDRESS: [u8; 32] = [0; 32];

const NO_WASM: bool = true;

/// Error returned as a result of a failed genesis process.
#[derive(Clone, Debug)]
pub enum GenesisError {
    /// Error creating a runtime.
    StateUninitialized,
    /// Error obtaining the mint's contract key.
    InvalidMintKey,
    /// Missing mint contract.
    MissingMintContract,
    /// Unexpected stored value variant.
    UnexpectedStoredValue,
    /// Error executing the mint system contract.
    MintError(mint::Error),
    /// Error converting a [`CLValue`] to a concrete type.
    CLValue(String),
    /// Specified validator does not exist among the genesis accounts.
    OrphanedDelegator {
        /// Validator's public key.
        validator_public_key: PublicKey,
        /// Delegator's public key.
        delegator_public_key: PublicKey,
    },
    /// Duplicated delegator entry found for a given validator.
    DuplicatedDelegatorEntry {
        /// Validator's public key.
        validator_public_key: PublicKey,
        /// Delegator's public key.
        delegator_public_key: PublicKey,
    },
    /// Delegation rate outside the allowed range.
    InvalidDelegationRate {
        /// Delegator's public key.
        public_key: PublicKey,
        /// Invalid delegation rate specified in the genesis account entry.
        delegation_rate: DelegationRate,
    },
    /// Invalid bond amount in a genesis account.
    InvalidBondAmount {
        /// Validator's public key.
        public_key: PublicKey,
    },
    /// Invalid delegated amount in a genesis account.
    InvalidDelegatedAmount {
        /// Delegator's public key.
        public_key: PublicKey,
    },
    /// Failed to create system registry.
    FailedToCreateSystemRegistry,
    /// Missing system contract hash.
    MissingSystemContractHash(String),
    /// Invalid number of validator slots configured.
    InvalidValidatorSlots {
        /// Number of validators in the genesis config.
        validators: usize,
        /// Number of validator slots specified.
        validator_slots: u32,
    },
    /// The chainspec registry is missing a required entry.
    MissingChainspecRegistryEntry,
    /// Duplicated administrator entry.
    ///
    /// This error can occur only on some private chains.
    DuplicatedAdministratorEntry,
    /// A bytesrepr Error.
    Bytesrepr(bytesrepr::Error),
    /// Genesis process requires initial accounts.
    MissingGenesisAccounts,
    /// A tracking copy error.
    TrackingCopy(TrackingCopyError),
}

impl fmt::Display for GenesisError {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "GenesisError: {:?}", self)
    }
}

/// State for genesis installer.
pub enum GenesisInstaller<S>
where
    S: StateProvider,
{
    /// Install genesis using the Accounts/Contracts model.
    AccountContract(AccountContractInstaller<S>),
    /// Install genesis using the Addressable Entity model.
    Entity(EntityGenesisInstaller<S>),
}
impl<S> GenesisInstaller<S>
where
    S: StateProvider,
{
    /// Ctor.
    pub fn new(
        genesis_config_hash: Digest,
        protocol_version: ProtocolVersion,
        config: GenesisConfig,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
    ) -> Self {
        if config.enable_entity() {
            GenesisInstaller::Entity(EntityGenesisInstaller::new(
                genesis_config_hash,
                protocol_version,
                config,
                tracking_copy,
            ))
        } else {
            GenesisInstaller::AccountContract(AccountContractInstaller::new(
                genesis_config_hash,
                protocol_version,
                config,
                tracking_copy,
            ))
        }
    }

    /// Finalize genesis.
    pub fn finalize(self) -> Effects {
        match self {
            GenesisInstaller::AccountContract(installer) => installer.finalize(),
            GenesisInstaller::Entity(installer) => installer.finalize(),
        }
    }

    /// Performs a complete system installation.
    pub fn install(
        &mut self,
        chainspec_registry: ChainspecRegistry,
    ) -> Result<(), Box<GenesisError>> {
        match self {
            GenesisInstaller::AccountContract(installer) => installer.install(chainspec_registry),
            GenesisInstaller::Entity(installer) => installer.install(chainspec_registry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::AsymmetricType;
    use rand::RngCore;

    use casper_types::{bytesrepr, SecretKey};

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = rand::thread_rng();
        let genesis_account: GenesisAccount = rng.gen();
        bytesrepr::test_serialization_roundtrip(&genesis_account);
    }

    #[test]
    fn system_account_bytesrepr_roundtrip() {
        let genesis_account = GenesisAccount::system();

        bytesrepr::test_serialization_roundtrip(&genesis_account);
    }

    #[test]
    fn genesis_account_bytesrepr_roundtrip() {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes[..]);
        let secret_key = SecretKey::ed25519_from_bytes(bytes).unwrap();
        let public_key: PublicKey = PublicKey::from(&secret_key);

        let genesis_account_1 = GenesisAccount::account(public_key.clone(), Motes::new(100), None);

        bytesrepr::test_serialization_roundtrip(&genesis_account_1);

        let genesis_account_2 =
            GenesisAccount::account(public_key, Motes::new(100), Some(rng.gen()));

        bytesrepr::test_serialization_roundtrip(&genesis_account_2);
    }

    #[test]
    fn delegator_bytesrepr_roundtrip() {
        let mut rng = rand::thread_rng();
        let mut validator_bytes = [0u8; 32];
        let mut delegator_bytes = [0u8; 32];
        rng.fill_bytes(&mut validator_bytes[..]);
        rng.fill_bytes(&mut delegator_bytes[..]);
        let validator_secret_key = SecretKey::ed25519_from_bytes(validator_bytes).unwrap();
        let delegator_secret_key = SecretKey::ed25519_from_bytes(delegator_bytes).unwrap();

        let validator_public_key = PublicKey::from(&validator_secret_key);
        let delegator_public_key = PublicKey::from(&delegator_secret_key);

        let genesis_account = GenesisAccount::delegator(
            validator_public_key,
            delegator_public_key,
            Motes::new(100),
            Motes::zero(),
        );

        bytesrepr::test_serialization_roundtrip(&genesis_account);
    }

    #[test]
    fn administrator_account_bytesrepr_roundtrip() {
        let administrator_account = AdministratorAccount::new(
            PublicKey::ed25519_from_bytes([123u8; 32]).unwrap(),
            Motes::new(U512::MAX),
        );
        bytesrepr::test_serialization_roundtrip(&administrator_account);
    }
}
