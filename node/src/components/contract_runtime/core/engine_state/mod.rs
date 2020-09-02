pub mod deploy_item;
pub mod engine_config;
mod error;
pub mod executable_deploy_item;
pub mod execute_request;
pub mod execution_effect;
pub mod execution_result;
pub mod genesis;
pub mod op;
pub mod query;
pub mod run_genesis_request;
pub mod system_contract_cache;
mod transfer;
pub mod upgrade;

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use num_traits::Zero;
use parity_wasm::elements::Module;
use tracing::{debug, warn};

use casper_types::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
    contracts::{NamedKeys, ENTRY_POINT_NAME_INSTALL, UPGRADE_ENTRY_POINT_NAME},
    runtime_args,
    system_contract_errors::mint,
    AccessRights, BlockTime, Contract, ContractHash, ContractPackage, ContractPackageHash,
    ContractVersionKey, EntryPoint, EntryPointType, Key, Phase, ProtocolVersion, RuntimeArgs, URef,
    U512,
};

use self::{
    deploy_item::DeployItem,
    executable_deploy_item::ExecutableDeployItem,
    execute_request::ExecuteRequest,
    execution_result::{ExecutionResult, ForcedTransferResult},
    genesis::{ExecConfig, GenesisResult, POS_PAYMENT_PURSE, POS_REWARDS_PURSE},
    query::{QueryRequest, QueryResult},
    system_contract_cache::SystemContractCache,
    transfer::TransferTargetMode,
    upgrade::{UpgradeConfig, UpgradeResult},
};
pub use self::{
    engine_config::EngineConfig,
    error::{Error, RootNotFound},
    transfer::TransferRuntimeArgsBuilder,
};
use crate::{
    components::{
        chainspec_loader::{Chainspec, GenesisAccount},
        contract_runtime::{
            core::{
                execution::{
                    self, AddressGenerator, AddressGeneratorBuilder, DirectSystemContractCall,
                    Executor,
                },
                tracking_copy::{TrackingCopy, TrackingCopyExt},
            },
            shared::{
                account::Account,
                additive_map::AdditiveMap,
                gas::Gas,
                newtypes::{Blake2bHash, CorrelationId},
                stored_value::StoredValue,
                transform::Transform,
                wasm_costs::WasmCosts,
                wasm_prep::{self, Preprocessor},
            },
            storage::{
                global_state::{CommitResult, StateProvider},
                protocol_data::ProtocolData,
            },
        },
    },
    crypto::{asymmetric_key::PublicKey, hash},
    types::Motes,
};
use execution_result::ExecutionResults;

// TODO?: MAX_PAYMENT && CONV_RATE values are currently arbitrary w/ real values
// TBD gas * CONV_RATE = motes
pub const MAX_PAYMENT: u64 = 10_000_000;
pub const CONV_RATE: u64 = 10;

pub const SYSTEM_ACCOUNT_ADDR: AccountHash = AccountHash::new([0u8; 32]);

const GENESIS_INITIAL_BLOCKTIME: u64 = 0;
const ARG_AMOUNT: &str = "amount";

#[derive(Debug)]
pub struct EngineState<S> {
    config: EngineConfig,
    system_contract_cache: SystemContractCache,
    state: S,
}

#[derive(Clone, Debug)]
pub enum GetModuleResult {
    Session {
        module: Module,
        contract_package: ContractPackage,
        entry_point: EntryPoint,
    },
    Contract {
        // Contract hash
        base_key: Key,
        module: Module,
        contract: Contract,
        contract_package: ContractPackage,
        entry_point: EntryPoint,
    },
}

impl GetModuleResult {
    pub fn take_module(self) -> Module {
        match self {
            GetModuleResult::Session { module, .. } => module,
            GetModuleResult::Contract { module, .. } => module,
        }
    }
}

impl<S> EngineState<S>
where
    S: StateProvider,
    S::Error: Into<execution::Error>,
{
    pub fn new(state: S, config: EngineConfig) -> EngineState<S> {
        let system_contract_cache = Default::default();
        EngineState {
            config,
            system_contract_cache,
            state,
        }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn wasm_costs(
        &self,
        protocol_version: ProtocolVersion,
    ) -> Result<Option<WasmCosts>, Error> {
        match self.get_protocol_data(protocol_version)? {
            Some(protocol_data) => Ok(Some(*protocol_data.wasm_costs())),
            None => Ok(None),
        }
    }

    pub fn get_protocol_data(
        &self,
        protocol_version: ProtocolVersion,
    ) -> Result<Option<ProtocolData>, Error> {
        match self.state.get_protocol_data(protocol_version) {
            Ok(Some(protocol_data)) => Ok(Some(protocol_data)),
            Err(error) => Err(Error::Exec(error.into())),
            _ => Ok(None),
        }
    }

    pub(crate) fn commit_genesis(&self, chainspec: Chainspec) -> Result<GenesisResult, Error> {
        let correlation_id = CorrelationId::new();
        let serialized_chainspec = rmp_serde::to_vec(&chainspec)?;
        let genesis_config_hash = hash::hash(&serialized_chainspec);
        let protocol_version = ProtocolVersion::from_parts(
            chainspec.genesis.protocol_version.major as u32,
            chainspec.genesis.protocol_version.minor as u32,
            chainspec.genesis.protocol_version.patch as u32,
        );

        let ee_config = ExecConfig::new(
            chainspec.genesis.mint_installer_bytes,
            chainspec.genesis.pos_installer_bytes,
            chainspec.genesis.standard_payment_installer_bytes,
            chainspec.genesis.auction_installer_bytes,
            chainspec.genesis.accounts,
            chainspec.genesis.costs,
        );
        self.commit_genesis_old(
            correlation_id,
            genesis_config_hash,
            protocol_version,
            &ee_config,
        )
    }

    pub fn commit_genesis_old(
        &self,
        correlation_id: CorrelationId,
        genesis_config_hash: Blake2bHash,
        protocol_version: ProtocolVersion,
        ee_config: &ExecConfig,
    ) -> Result<GenesisResult, Error> {
        // Preliminaries
        let executor = Executor::new(self.config);
        let blocktime = BlockTime::new(GENESIS_INITIAL_BLOCKTIME);
        let gas_limit = Gas::new(std::u64::MAX.into());
        let phase = Phase::System;

        let initial_root_hash = self.state.empty_root();
        let wasm_costs = ee_config.wasm_costs();
        let preprocessor = Preprocessor::new(wasm_costs);

        // Spec #3: Create "virtual system account" object.
        let mut virtual_system_account = {
            let named_keys = NamedKeys::new();
            let purse = URef::new(Default::default(), AccessRights::READ_ADD_WRITE);
            Account::create(SYSTEM_ACCOUNT_ADDR, named_keys, purse)
        };

        // Spec #4: Create a runtime.
        let tracking_copy = match self.tracking_copy(initial_root_hash) {
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
            Ok(None) => panic!("state has not been initialized properly"),
            Err(error) => return Err(error),
        };

        // Persist the "virtual system account".  It will get overwritten with the actual system
        // account below.
        let key = Key::Account(SYSTEM_ACCOUNT_ADDR);
        let value = {
            let virtual_system_account = virtual_system_account.clone();
            StoredValue::Account(virtual_system_account)
        };

        tracking_copy.borrow_mut().write(key, value);

        // Spec #4A: random number generator is seeded from the hash of GenesisConfig.name
        // Updated: random number generator is seeded from genesis_config_hash from the RunGenesis
        // RPC call

        let hash_address_generator = {
            let generator = AddressGenerator::new(genesis_config_hash.as_ref(), phase);
            Rc::new(RefCell::new(generator))
        };
        let uref_address_generator = {
            let generator = AddressGenerator::new(genesis_config_hash.as_ref(), phase);
            Rc::new(RefCell::new(generator))
        };

        // Spec #6: Compute initially bonded validators as the contents of accounts_path
        // filtered to non-zero staked amounts.
        let bonded_validators: BTreeMap<AccountHash, U512> = ee_config
            .get_bonded_validators()
            .map(|(k, v)| (k, v.value()))
            .collect();

        // Spec #5: Execute the wasm code from the mint installer bytes
        let (mint_package_hash, mint_hash): (ContractPackageHash, ContractHash) = {
            let mint_installer_bytes = ee_config.mint_installer_bytes();
            let mint_installer_module = preprocessor.preprocess(mint_installer_bytes)?;
            let args = RuntimeArgs::new();
            let authorization_keys: BTreeSet<AccountHash> = BTreeSet::new();
            let install_deploy_hash = genesis_config_hash.to_bytes();
            let hash_address_generator = Rc::clone(&hash_address_generator);
            let uref_address_generator = Rc::clone(&uref_address_generator);
            let tracking_copy = Rc::clone(&tracking_copy);
            let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);
            let protocol_data = ProtocolData::default();

            executor.exec_wasm_direct(
                mint_installer_module,
                ENTRY_POINT_NAME_INSTALL,
                args,
                &mut virtual_system_account,
                authorization_keys,
                blocktime,
                install_deploy_hash,
                gas_limit,
                hash_address_generator,
                uref_address_generator,
                protocol_version,
                correlation_id,
                tracking_copy,
                phase,
                protocol_data,
                system_contract_cache,
            )?
        };

        // Spec #7: Execute pos installer wasm code, passing the initially bonded validators as an
        // argument
        let (_proof_of_stake_package_hash, proof_of_stake_hash): (
            ContractPackageHash,
            ContractHash,
        ) = {
            let tracking_copy = Rc::clone(&tracking_copy);
            let hash_address_generator = Rc::clone(&hash_address_generator);
            let uref_address_generator = Rc::clone(&uref_address_generator);
            let install_deploy_hash = genesis_config_hash.to_bytes();
            let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);

            // Constructs a partial protocol data with already known uref to pass the validation
            // step
            let partial_protocol_data = ProtocolData::partial_with_mint(mint_hash);

            let proof_of_stake_installer_bytes = ee_config.proof_of_stake_installer_bytes();
            let proof_of_stake_installer_module =
                preprocessor.preprocess(proof_of_stake_installer_bytes)?;
            let args = runtime_args! {
                "mint_contract_package_hash" => mint_package_hash,
                "genesis_validators" => bonded_validators,
            };
            let authorization_keys: BTreeSet<AccountHash> = BTreeSet::new();

            executor.exec_wasm_direct(
                proof_of_stake_installer_module,
                ENTRY_POINT_NAME_INSTALL,
                args,
                &mut virtual_system_account,
                authorization_keys,
                blocktime,
                install_deploy_hash,
                gas_limit,
                hash_address_generator,
                uref_address_generator,
                protocol_version,
                correlation_id,
                tracking_copy,
                phase,
                partial_protocol_data,
                system_contract_cache,
            )?
        };

        // Execute standard payment installer wasm code
        //
        // Note: this deviates from the implementation strategy described in the original
        // specification.
        let protocol_data = ProtocolData::partial_without_standard_payment(
            wasm_costs,
            mint_hash,
            proof_of_stake_hash,
        );

        let standard_payment_hash: ContractHash = {
            let standard_payment_installer_bytes = {
                // NOTE: Before integration node wasn't updated to pass the bytes, so we were
                // bundling it. This debug_assert can be removed once integration with genesis
                // works.
                debug_assert!(
                    !ee_config.standard_payment_installer_bytes().is_empty(),
                    "Caller is required to pass the standard_payment_installer bytes"
                );
                &ee_config.standard_payment_installer_bytes()
            };

            let standard_payment_installer_module =
                preprocessor.preprocess(standard_payment_installer_bytes)?;
            let args = RuntimeArgs::new();
            let authorization_keys = BTreeSet::new();
            let install_deploy_hash = genesis_config_hash.to_bytes();
            let hash_address_generator = Rc::clone(&hash_address_generator);
            let uref_address_generator = Rc::clone(&uref_address_generator);
            let tracking_copy = Rc::clone(&tracking_copy);
            let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);

            executor.exec_wasm_direct(
                standard_payment_installer_module,
                ENTRY_POINT_NAME_INSTALL,
                args,
                &mut virtual_system_account,
                authorization_keys,
                blocktime,
                install_deploy_hash,
                gas_limit,
                hash_address_generator,
                uref_address_generator,
                protocol_version,
                correlation_id,
                tracking_copy,
                phase,
                protocol_data,
                system_contract_cache,
            )?
        };

        let auction_hash: ContractHash = {
            let bonded_validators: BTreeMap<casper_types::PublicKey, U512> = ee_config
                .accounts()
                .iter()
                .filter_map(|genesis_account| {
                    if genesis_account.is_genesis_validator() {
                        Some((
                            casper_types::PublicKey::from(
                                genesis_account
                                    .public_key()
                                    .expect("should have public key"),
                            ),
                            genesis_account.bonded_amount().value(),
                        ))
                    } else {
                        None
                    }
                })
                .collect();

            let auction_installer_bytes = {
                // NOTE: Before integration node wasn't updated to pass the bytes, so we were
                // bundling it. This debug_assert can be removed once integration with genesis
                // works.
                debug_assert!(
                    !ee_config.auction_installer_bytes().is_empty(),
                    "Caller is required to pass the auction_installer bytes"
                );
                &ee_config.auction_installer_bytes()
            };

            let auction_installer_module = preprocessor.preprocess(auction_installer_bytes)?;
            let args = runtime_args! {
                "mint_contract_package_hash" => mint_package_hash,
                "genesis_validators" => bonded_validators,
            };
            let authorization_keys = BTreeSet::new();
            let install_deploy_hash = genesis_config_hash.to_bytes();
            let hash_address_generator = Rc::clone(&hash_address_generator);
            let uref_address_generator = Rc::clone(&uref_address_generator);
            let tracking_copy = Rc::clone(&tracking_copy);
            let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);

            executor.exec_wasm_direct(
                auction_installer_module,
                ENTRY_POINT_NAME_INSTALL,
                args,
                &mut virtual_system_account,
                authorization_keys,
                blocktime,
                install_deploy_hash,
                gas_limit,
                hash_address_generator,
                uref_address_generator,
                protocol_version,
                correlation_id,
                tracking_copy,
                phase,
                protocol_data,
                system_contract_cache,
            )?
        };

        // Spec #2: Associate given CostTable with given ProtocolVersion.
        let protocol_data = ProtocolData::new(
            wasm_costs,
            mint_hash,
            proof_of_stake_hash,
            standard_payment_hash,
            auction_hash,
        );

        self.state
            .put_protocol_data(protocol_version, &protocol_data)
            .map_err(Into::into)?;

        //
        // NOTE: The following stanzas deviate from the implementation strategy described in the
        // original specification.
        //
        // It has the following benefits over that approach:
        // * It does not make an intermediate commit
        // * The system account never holds funds
        // * Similarly, the system account does not need to be handled differently than a normal
        //   account (with the exception of its known keys)
        //
        // Create known keys for chainspec accounts
        let account_named_keys = NamedKeys::new();

        // Create accounts
        {
            // Collect chainspec accounts and their known keys with the genesis account and its
            // known keys
            let accounts = {
                let mut ret: Vec<(GenesisAccount, NamedKeys)> = ee_config
                    .accounts()
                    .to_vec()
                    .into_iter()
                    .map(|account| (account, account_named_keys.clone()))
                    .collect();
                let system_account = GenesisAccount::system(Motes::zero(), Motes::zero());
                ret.push((system_account, virtual_system_account.named_keys().clone()));
                ret
            };

            // Get the mint module
            let module = {
                let contract = tracking_copy
                    .borrow_mut()
                    .get_contract(correlation_id, mint_hash)?;

                let contract_wasm = tracking_copy
                    .borrow_mut()
                    .get_contract_wasm(correlation_id, contract.contract_wasm_hash())?;
                let bytes = contract_wasm.bytes();
                wasm_prep::deserialize(&bytes)?
            };
            // For each account...
            for (account, named_keys) in accounts.into_iter() {
                let module = module.clone();
                let args = runtime_args! {
                    ARG_AMOUNT => account.balance().value(),
                };
                let tracking_copy_exec = Rc::clone(&tracking_copy);
                let tracking_copy_write = Rc::clone(&tracking_copy);
                let mut named_keys_exec = NamedKeys::new();
                let base_key = mint_hash;
                let authorization_keys: BTreeSet<AccountHash> = BTreeSet::new();
                let account_hash = account
                    .public_key()
                    .as_ref()
                    .map(PublicKey::to_account_hash)
                    .unwrap_or(SYSTEM_ACCOUNT_ADDR);
                let purse_creation_deploy_hash = account_hash.value();
                let hash_address_generator = Rc::clone(&hash_address_generator);
                let uref_address_generator = {
                    let generator = AddressGeneratorBuilder::new()
                        .seed_with(genesis_config_hash.as_ref())
                        .seed_with(&account_hash.to_bytes()?)
                        .seed_with(&[phase as u8])
                        .build();
                    Rc::new(RefCell::new(generator))
                };
                let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);

                let mint_result: Result<URef, mint::Error> = {
                    // ...call the Mint's "mint" endpoint to create purse with tokens...
                    let (_instance, mut runtime) = executor.create_runtime(
                        module,
                        EntryPointType::Contract,
                        args.clone(),
                        &mut named_keys_exec,
                        Default::default(),
                        base_key.into(),
                        &virtual_system_account,
                        authorization_keys,
                        blocktime,
                        purse_creation_deploy_hash,
                        gas_limit,
                        hash_address_generator,
                        uref_address_generator,
                        protocol_version,
                        correlation_id,
                        tracking_copy_exec,
                        phase,
                        protocol_data,
                        system_contract_cache,
                    )?;

                    runtime
                        .call_versioned_contract(
                            mint_package_hash,
                            Some(1),
                            "mint".to_string(),
                            args,
                        )?
                        .into_t::<Result<URef, mint::Error>>()
                        .expect("should convert")
                };

                // ...and write that account to global state...
                let key = Key::Account(account_hash);
                let value = {
                    let main_purse = mint_result?;
                    StoredValue::Account(Account::create(account_hash, named_keys, main_purse))
                };

                tracking_copy_write.borrow_mut().write(key, value);
            }
        }
        // Spec #15: Commit the transforms.
        let effects = tracking_copy.borrow().effect();

        let commit_result = self
            .state
            .commit(
                correlation_id,
                initial_root_hash,
                effects.transforms.to_owned(),
            )
            .map_err(Into::into)?;

        // Return the result
        let genesis_result = GenesisResult::from_commit_result(commit_result, effects);

        Ok(genesis_result)
    }

    pub fn commit_upgrade(
        &self,
        correlation_id: CorrelationId,
        upgrade_config: UpgradeConfig,
    ) -> Result<UpgradeResult, Error> {
        // per specification:
        // https://casperlabs.atlassian.net/wiki/spaces/EN/pages/139854367/Upgrading+System+Contracts+Specification

        // 3.1.1.1.1.1 validate pre state hash exists
        // 3.1.2.1 get a tracking_copy at the provided pre_state_hash
        let pre_state_hash = upgrade_config.pre_state_hash();
        let tracking_copy = match self.tracking_copy(pre_state_hash)? {
            Some(tracking_copy) => Rc::new(RefCell::new(tracking_copy)),
            None => return Ok(UpgradeResult::RootNotFound),
        };

        // 3.1.1.1.1.2 current protocol version is required
        let current_protocol_version = upgrade_config.current_protocol_version();
        let current_protocol_data = match self.state.get_protocol_data(current_protocol_version) {
            Ok(Some(protocol_data)) => protocol_data,
            Ok(None) => {
                return Err(Error::InvalidProtocolVersion(current_protocol_version));
            }
            Err(error) => {
                return Err(Error::Exec(error.into()));
            }
        };

        // 3.1.1.1.1.3 activation point is not currently used by EE; skipping
        // 3.1.1.1.1.4 upgrade point protocol version validation
        let new_protocol_version = upgrade_config.new_protocol_version();

        let upgrade_check_result =
            current_protocol_version.check_next_version(&new_protocol_version);

        if upgrade_check_result.is_invalid() {
            return Err(Error::InvalidProtocolVersion(new_protocol_version));
        }

        // 3.1.1.1.1.6 resolve wasm CostTable for new protocol version
        let new_wasm_costs = match upgrade_config.wasm_costs() {
            Some(new_wasm_costs) => new_wasm_costs,
            None => *current_protocol_data.wasm_costs(),
        };

        // 3.1.2.2 persist wasm CostTable
        let mut new_protocol_data = ProtocolData::new(
            new_wasm_costs,
            current_protocol_data.mint(),
            current_protocol_data.proof_of_stake(),
            current_protocol_data.standard_payment(),
            current_protocol_data.auction(),
        );

        self.state
            .put_protocol_data(new_protocol_version, &new_protocol_data)
            .map_err(Into::into)?;

        // 3.1.1.1.1.5 upgrade installer is optional except on major version upgrades
        match upgrade_config.upgrade_installer_bytes() {
            None if upgrade_check_result.is_code_required() => {
                // 3.1.1.1.1.5 code is required for major version bump
                return Err(Error::InvalidUpgradeConfig);
            }
            None => {
                // optional for patch/minor bumps
            }
            Some(bytes) => {
                // 3.1.2.3 execute upgrade installer if one is provided

                // preprocess installer module
                let upgrade_installer_module = {
                    let preprocessor = Preprocessor::new(new_wasm_costs);
                    preprocessor.preprocess(bytes)?
                };

                // currently there are no expected args for an upgrade installer but args are
                // supported
                let args = match upgrade_config.upgrade_installer_args() {
                    Some(args) => {
                        bytesrepr::deserialize(args.to_vec()).expect("should deserialize")
                    }
                    None => RuntimeArgs::new(),
                };

                // execute as system account
                let mut system_account = {
                    let key = Key::Account(SYSTEM_ACCOUNT_ADDR);
                    match tracking_copy.borrow_mut().read(correlation_id, &key) {
                        Ok(Some(StoredValue::Account(account))) => account,
                        Ok(_) => panic!("system account must exist"),
                        Err(error) => return Err(Error::Exec(error.into())),
                    }
                };

                let authorization_keys = {
                    let mut ret = BTreeSet::new();
                    ret.insert(SYSTEM_ACCOUNT_ADDR);
                    ret
                };

                let blocktime = BlockTime::default();

                let deploy_hash = {
                    // seeds address generator w/ protocol version
                    let bytes: Vec<u8> = upgrade_config
                        .new_protocol_version()
                        .value()
                        .into_bytes()?
                        .to_vec();
                    hash::hash(&bytes).to_bytes()
                };

                // upgrade has no gas limit; approximating with MAX
                let gas_limit = Gas::new(std::u64::MAX.into());
                let phase = Phase::System;
                let hash_address_generator = {
                    let generator = AddressGenerator::new(pre_state_hash.as_ref(), phase);
                    Rc::new(RefCell::new(generator))
                };
                let uref_address_generator = {
                    let generator = AddressGenerator::new(pre_state_hash.as_ref(), phase);
                    Rc::new(RefCell::new(generator))
                };
                let tracking_copy = Rc::clone(&tracking_copy);
                let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);

                let executor = Executor::new(self.config);

                let result: BTreeMap<ContractHash, ContractHash> = executor.exec_wasm_direct(
                    upgrade_installer_module,
                    UPGRADE_ENTRY_POINT_NAME,
                    args,
                    &mut system_account,
                    authorization_keys,
                    blocktime,
                    deploy_hash,
                    gas_limit,
                    hash_address_generator,
                    uref_address_generator,
                    new_protocol_version,
                    correlation_id,
                    Rc::clone(&tracking_copy),
                    phase,
                    new_protocol_data,
                    system_contract_cache,
                )?;

                if !new_protocol_data.update_from(result) {
                    return Err(Error::InvalidUpgradeResult);
                } else {
                    self.state
                        .put_protocol_data(new_protocol_version, &new_protocol_data)
                        .map_err(Into::into)?;
                }
            }
        }

        let effects = tracking_copy.borrow().effect();

        // commit
        let commit_result = self
            .state
            .commit(
                correlation_id,
                pre_state_hash,
                effects.transforms.to_owned(),
            )
            .map_err(Into::into)?;

        // return result and effects
        Ok(UpgradeResult::from_commit_result(commit_result, effects))
    }

    pub fn tracking_copy(
        &self,
        hash: Blake2bHash,
    ) -> Result<Option<TrackingCopy<S::Reader>>, Error> {
        match self.state.checkout(hash).map_err(Into::into)? {
            Some(tc) => Ok(Some(TrackingCopy::new(tc))),
            None => Ok(None),
        }
    }

    pub fn run_query(
        &self,
        correlation_id: CorrelationId,
        query_request: QueryRequest,
    ) -> Result<QueryResult, Error> {
        let tracking_copy = match self.tracking_copy(query_request.state_hash())? {
            Some(tracking_copy) => Rc::new(RefCell::new(tracking_copy)),
            None => return Ok(QueryResult::RootNotFound),
        };

        let tracking_copy = tracking_copy.borrow();

        Ok(tracking_copy
            .query(correlation_id, query_request.key(), query_request.path())
            .map_err(|err| Error::Exec(err.into()))?
            .into())
    }

    pub fn run_execute(
        &self,
        correlation_id: CorrelationId,
        mut exec_request: ExecuteRequest,
    ) -> Result<ExecutionResults, RootNotFound> {
        // TODO: do not unwrap
        let wasm_costs = self
            .wasm_costs(exec_request.protocol_version)
            .unwrap()
            .unwrap();
        let executor = Executor::new(self.config);
        let preprocessor = Preprocessor::new(wasm_costs);

        let deploys = exec_request.take_deploys();
        let mut results = ExecutionResults::with_capacity(deploys.len());

        for deploy_item in deploys {
            let result = match deploy_item {
                Err(exec_result) => Ok(exec_result),
                Ok(deploy_item) => match deploy_item.session {
                    ExecutableDeployItem::Transfer { .. } => self.transfer(
                        correlation_id,
                        &executor,
                        &preprocessor,
                        exec_request.protocol_version,
                        exec_request.parent_state_hash,
                        BlockTime::new(exec_request.block_time),
                        deploy_item,
                    ),
                    _ => self.deploy(
                        correlation_id,
                        &executor,
                        &preprocessor,
                        exec_request.protocol_version,
                        exec_request.parent_state_hash,
                        BlockTime::new(exec_request.block_time),
                        deploy_item,
                    ),
                },
            };
            match result {
                Ok(result) => results.push_back(result),
                Err(error) => {
                    return Err(error);
                }
            };
        }

        Ok(results)
    }

    pub fn get_module(
        &self,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
        deploy_item: &ExecutableDeployItem,
        account: &Account,
        correlation_id: CorrelationId,
        preprocessor: &Preprocessor,
        protocol_version: &ProtocolVersion,
    ) -> Result<GetModuleResult, Error> {
        let (contract_package, contract, base_key) = match deploy_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, .. } => {
                let module = preprocessor.preprocess(&module_bytes)?;
                return Ok(GetModuleResult::Session {
                    module,
                    contract_package: ContractPackage::default(),
                    entry_point: EntryPoint::default(),
                });
            }
            ExecutableDeployItem::StoredContractByHash { .. }
            | ExecutableDeployItem::StoredContractByName { .. } => {
                let stored_contract_key = deploy_item.to_contract_hash_key(&account)?.unwrap();

                let contract = tracking_copy
                    .borrow_mut()
                    .get_contract(correlation_id, stored_contract_key.into_hash().unwrap())?;

                if !contract.is_compatible_protocol_version(*protocol_version) {
                    let exec_error = execution::Error::IncompatibleProtocolMajorVersion {
                        expected: protocol_version.value().major,
                        actual: contract.protocol_version().value().major,
                    };
                    return Err(error::Error::Exec(exec_error));
                }

                let contract_package = tracking_copy
                    .borrow_mut()
                    .get_contract_package(correlation_id, contract.contract_package_hash())?;

                (contract_package, contract, stored_contract_key)
            }
            ExecutableDeployItem::StoredVersionedContractByName { version, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { version, .. } => {
                let contract_package_key = deploy_item.to_contract_hash_key(&account)?.unwrap();
                let contract_package_hash = contract_package_key.into_seed();

                let contract_package = tracking_copy
                    .borrow_mut()
                    .get_contract_package(correlation_id, contract_package_hash)?;

                let maybe_version_key =
                    version.map(|ver| ContractVersionKey::new(protocol_version.value().major, ver));

                let contract_version_key = maybe_version_key
                    .or_else(|| contract_package.current_contract_version())
                    .ok_or_else(|| {
                        error::Error::Exec(execution::Error::NoActiveContractVersions(
                            contract_package_hash,
                        ))
                    })?;

                if !contract_package.is_version_enabled(contract_version_key) {
                    return Err(error::Error::Exec(
                        execution::Error::InvalidContractVersion(contract_version_key),
                    ));
                }

                let contract_hash = *contract_package
                    .lookup_contract_hash(contract_version_key)
                    .ok_or_else(|| {
                        error::Error::Exec(execution::Error::InvalidContractVersion(
                            contract_version_key,
                        ))
                    })?;

                let contract = tracking_copy
                    .borrow_mut()
                    .get_contract(correlation_id, contract_hash)?;

                (contract_package, contract, contract_package_key)
            }
            ExecutableDeployItem::Transfer { .. } => {
                return Err(error::Error::InvalidDeployItemVariant(String::from(
                    "Transfer",
                )))
            }
        };

        let entry_point_name = deploy_item.entry_point_name();

        let entry_point = contract
            .entry_point(entry_point_name)
            .cloned()
            .ok_or_else(|| {
                error::Error::Exec(execution::Error::NoSuchMethod(entry_point_name.to_owned()))
            })?;

        let contract_wasm = tracking_copy
            .borrow_mut()
            .get_contract_wasm(correlation_id, contract.contract_wasm_hash())?;

        let module = wasm_prep::deserialize(contract_wasm.bytes())?;

        match entry_point.entry_point_type() {
            EntryPointType::Session => Ok(GetModuleResult::Session {
                module,
                contract_package,
                entry_point,
            }),
            EntryPointType::Contract => Ok(GetModuleResult::Contract {
                module,
                base_key,
                contract,
                contract_package,
                entry_point,
            }),
        }
    }

    fn get_module_from_contract_hash(
        &self,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
        contract_hash: ContractHash,
        correlation_id: CorrelationId,
        protocol_version: &ProtocolVersion,
    ) -> Result<Module, Error> {
        let contract = tracking_copy
            .borrow_mut()
            .get_contract(correlation_id, contract_hash)?;

        // A contract may only call a stored contract that has the same protocol major version
        // number.
        if !contract.is_compatible_protocol_version(*protocol_version) {
            let exec_error = execution::Error::IncompatibleProtocolMajorVersion {
                expected: protocol_version.value().major,
                actual: contract.protocol_version().value().major,
            };
            return Err(error::Error::Exec(exec_error));
        }

        let contract_wasm = tracking_copy
            .borrow_mut()
            .get_contract_wasm(correlation_id, contract.contract_wasm_hash())?;

        let module = wasm_prep::deserialize(contract_wasm.bytes())?;

        Ok(module)
    }

    fn get_authorized_account(
        &self,
        correlation_id: CorrelationId,
        account_hash: AccountHash,
        authorization_keys: &BTreeSet<AccountHash>,
        tracking_copy: Rc<RefCell<TrackingCopy<<S as StateProvider>::Reader>>>,
    ) -> Result<Account, Error> {
        let account: Account = match tracking_copy
            .borrow_mut()
            .get_account(correlation_id, account_hash)
        {
            Ok(account) => account,
            Err(_) => {
                return Err(error::Error::Authorization);
            }
        };

        // Authorize using provided authorization keys
        if !account.can_authorize(authorization_keys) {
            return Err(error::Error::Authorization);
        }

        // Check total key weight against deploy threshold
        if !account.can_deploy_with(authorization_keys) {
            return Err(execution::Error::DeploymentAuthorizationFailure.into());
        }

        Ok(account)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn transfer(
        &self,
        correlation_id: CorrelationId,
        executor: &Executor,
        preprocessor: &Preprocessor,
        protocol_version: ProtocolVersion,
        prestate_hash: Blake2bHash,
        blocktime: BlockTime,
        deploy_item: DeployItem,
    ) -> Result<ExecutionResult, RootNotFound> {
        let protocol_data = match self.state.get_protocol_data(protocol_version) {
            Ok(Some(protocol_data)) => protocol_data,
            Ok(None) => {
                let error = Error::InvalidProtocolVersion(protocol_version);
                return Ok(ExecutionResult::precondition_failure(error));
            }
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(Error::Exec(
                    error.into(),
                )));
            }
        };

        let tracking_copy = match self.tracking_copy(prestate_hash) {
            Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            Ok(None) => return Err(RootNotFound::new(prestate_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };

        let base_key = Key::Account(deploy_item.address);

        let account_public_key = match base_key.into_account() {
            Some(account_addr) => account_addr,
            None => {
                return Ok(ExecutionResult::precondition_failure(
                    error::Error::Authorization,
                ));
            }
        };

        let authorization_keys = deploy_item.authorization_keys;

        let account = match self.get_authorized_account(
            correlation_id,
            account_public_key,
            &authorization_keys,
            Rc::clone(&tracking_copy),
        ) {
            Ok(account) => account,
            Err(e) => return Ok(ExecutionResult::precondition_failure(e)),
        };

        let mint_contract = match tracking_copy
            .borrow_mut()
            .get_contract(correlation_id, protocol_data.mint())
        {
            Ok(contract) => contract,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };

        let mint_module = {
            let contract_wasm_hash = mint_contract.contract_wasm_hash();
            let use_system_contracts = self.config.use_system_contracts();
            match tracking_copy.borrow_mut().get_system_module(
                correlation_id,
                contract_wasm_hash,
                use_system_contracts,
                preprocessor,
            ) {
                Ok(module) => module,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            }
        };

        let mut named_keys = mint_contract.named_keys().to_owned();
        let mut extra_keys: Vec<Key> = vec![];
        let base_key = Key::from(protocol_data.mint());
        let gas_limit = Gas::new(U512::from(std::u64::MAX));

        let input_runtime_args = match deploy_item.session.into_runtime_args() {
            Ok(runtime_args) => runtime_args,
            Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
        };

        let mut runtime_args_builder = TransferRuntimeArgsBuilder::new(input_runtime_args);
        match runtime_args_builder.transfer_target_mode(correlation_id, Rc::clone(&tracking_copy)) {
            Ok(mode) => match mode {
                TransferTargetMode::Unknown | TransferTargetMode::PurseExists(_) => { /* noop */ }
                TransferTargetMode::CreateAccount(public_key) => {
                    let (maybe_uref, execution_result): (Option<URef>, ExecutionResult) = executor
                        .exec_system_contract(
                            DirectSystemContractCall::CreatePurse,
                            mint_module.clone(),
                            runtime_args! {}, // mint create takes no arguments
                            &mut named_keys,
                            Default::default(),
                            base_key,
                            &account,
                            authorization_keys.clone(),
                            blocktime,
                            deploy_item.deploy_hash,
                            gas_limit,
                            protocol_version,
                            correlation_id,
                            Rc::clone(&tracking_copy),
                            Phase::Session,
                            protocol_data,
                            SystemContractCache::clone(&self.system_contract_cache),
                        );
                    match maybe_uref {
                        Some(main_purse) => {
                            let new_account =
                                Account::create(public_key, Default::default(), main_purse);
                            extra_keys.push(Key::from(main_purse));
                            // write new account
                            tracking_copy
                                .borrow_mut()
                                .write(Key::Account(public_key), StoredValue::Account(new_account))
                        }
                        None => {
                            return Ok(execution_result);
                        }
                    }
                }
            },
            Err(error) => {
                return Ok(ExecutionResult::Failure {
                    error,
                    effect: Default::default(),
                    cost: Gas::default(),
                });
            }
        }

        let runtime_args =
            match runtime_args_builder.build(&account, correlation_id, Rc::clone(&tracking_copy)) {
                Ok(runtime_args) => runtime_args,
                Err(error) => {
                    return Ok(ExecutionResult::Failure {
                        error,
                        effect: Default::default(),
                        cost: Gas::default(),
                    });
                }
            };

        let (_, execution_result): (Option<Result<(), u8>>, ExecutionResult) = executor
            .exec_system_contract(
                DirectSystemContractCall::Transfer,
                mint_module,
                runtime_args,
                &mut named_keys,
                extra_keys.as_slice(),
                base_key,
                &account,
                authorization_keys,
                blocktime,
                deploy_item.deploy_hash,
                gas_limit,
                protocol_version,
                correlation_id,
                tracking_copy,
                Phase::Session,
                protocol_data,
                SystemContractCache::clone(&self.system_contract_cache),
            );

        Ok(execution_result)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn deploy(
        &self,
        correlation_id: CorrelationId,
        executor: &Executor,
        preprocessor: &Preprocessor,
        protocol_version: ProtocolVersion,
        prestate_hash: Blake2bHash,
        blocktime: BlockTime,
        deploy_item: DeployItem,
    ) -> Result<ExecutionResult, RootNotFound> {
        // spec: https://casperlabs.atlassian.net/wiki/spaces/EN/pages/123404576/Payment+code+execution+specification

        // Obtain current protocol data for given version
        // do this first, as there is no reason to proceed if protocol version is invalid
        let protocol_data = match self.state.get_protocol_data(protocol_version) {
            Ok(Some(protocol_data)) => protocol_data,
            Ok(None) => {
                let error = Error::InvalidProtocolVersion(protocol_version);
                return Ok(ExecutionResult::precondition_failure(error));
            }
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(Error::Exec(
                    error.into(),
                )));
            }
        };

        // Create tracking copy (which functions as a deploy context)
        // validation_spec_2: prestate_hash check
        // do this second; as there is no reason to proceed if the prestate hash is invalid
        let tracking_copy = match self.tracking_copy(prestate_hash) {
            Err(error) => return Ok(ExecutionResult::precondition_failure(error)),
            Ok(None) => return Err(RootNotFound::new(prestate_hash)),
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
        };

        let base_key = Key::Account(deploy_item.address);

        // Get addr bytes from `address` (which is actually a Key)
        // validation_spec_3: account validity
        let account_public_key = match base_key.into_account() {
            Some(account_addr) => account_addr,
            None => {
                return Ok(ExecutionResult::precondition_failure(
                    error::Error::Authorization,
                ));
            }
        };

        let authorization_keys = deploy_item.authorization_keys;

        // Get account from tracking copy
        // validation_spec_3: account validity
        let account = match self.get_authorized_account(
            correlation_id,
            account_public_key,
            &authorization_keys,
            Rc::clone(&tracking_copy),
        ) {
            Ok(account) => account,
            Err(e) => return Ok(ExecutionResult::precondition_failure(e)),
        };

        let session = deploy_item.session;
        let payment = deploy_item.payment;
        let deploy_hash = deploy_item.deploy_hash;

        // Create session code `A` from provided session bytes
        // validation_spec_1: valid wasm bytes
        // we do this upfront as there is no reason to continue if session logic is invalid
        let session_module = match self.get_module(
            Rc::clone(&tracking_copy),
            &session,
            &account,
            correlation_id,
            preprocessor,
            &protocol_version,
        ) {
            Ok(module) => module,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error));
            }
        };

        // Get mint system contract details
        // payment_code_spec_6: system contract validity
        let mint_hash = protocol_data.mint();

        let mint_contract = match tracking_copy
            .borrow_mut()
            .get_contract(correlation_id, mint_hash)
        {
            Ok(contract) => contract,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };

        // cache mint module
        if !self.system_contract_cache.has(mint_hash) {
            let mint_module = match tracking_copy.borrow_mut().get_system_module(
                correlation_id,
                mint_contract.contract_wasm_hash(),
                self.config.use_system_contracts(),
                preprocessor,
            ) {
                Ok(contract) => contract,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            };

            self.system_contract_cache.insert(mint_hash, mint_module);
        }

        // Get proof of stake system contract URef from account (an account on a
        // different network may have a pos contract other than the CLPoS)
        // payment_code_spec_6: system contract validity
        let proof_of_stake_hash = protocol_data.proof_of_stake();

        // Get proof of stake system contract details
        // payment_code_spec_6: system contract validity
        let proof_of_stake_contract = match tracking_copy
            .borrow_mut()
            .get_contract(correlation_id, proof_of_stake_hash)
        {
            Ok(contract) => contract,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };

        let proof_of_stake_module = match tracking_copy.borrow_mut().get_system_module(
            correlation_id,
            proof_of_stake_contract.contract_wasm_hash(),
            self.config.use_system_contracts(),
            preprocessor,
        ) {
            Ok(module) => module,
            Err(error) => {
                return Ok(ExecutionResult::precondition_failure(error.into()));
            }
        };

        // cache proof_of_stake module
        if !self.system_contract_cache.has(proof_of_stake_hash) {
            self.system_contract_cache
                .insert(proof_of_stake_hash, proof_of_stake_module.clone());
        }

        // Get account main purse balance key
        // validation_spec_5: account main purse minimum balance
        let account_main_purse_balance_key: Key = {
            let account_key = Key::URef(account.main_purse());
            match tracking_copy
                .borrow_mut()
                .get_purse_balance_key(correlation_id, account_key)
            {
                Ok(key) => key,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            }
        };

        // Get account main purse balance to enforce precondition and in case of forced
        // transfer validation_spec_5: account main purse minimum balance
        let account_main_purse_balance: Motes = match tracking_copy
            .borrow_mut()
            .get_purse_balance(correlation_id, account_main_purse_balance_key)
        {
            Ok(balance) => balance,
            Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
        };

        let max_payment_cost: Motes = Motes::new(U512::from(MAX_PAYMENT));

        // Enforce minimum main purse balance validation
        // validation_spec_5: account main purse minimum balance
        if account_main_purse_balance < max_payment_cost {
            return Ok(ExecutionResult::precondition_failure(
                Error::InsufficientPayment,
            ));
        }

        // Finalization is executed by system account (currently genesis account)
        // payment_code_spec_5: system executes finalization
        let system_account = Account::new(
            SYSTEM_ACCOUNT_ADDR,
            Default::default(),
            URef::new(Default::default(), AccessRights::READ_ADD_WRITE),
            Default::default(),
            Default::default(),
        );

        // [`ExecutionResultBuilder`] handles merging of multiple execution results
        let mut execution_result_builder = execution_result::ExecutionResultBuilder::new();

        // Execute provided payment code
        let payment_result = {
            // payment_code_spec_1: init pay environment w/ gas limit == (max_payment_cost /
            // conv_rate)
            let pay_gas_limit = Gas::from_motes(max_payment_cost, CONV_RATE).unwrap_or_default();

            let module_bytes_is_empty = match payment {
                ExecutableDeployItem::ModuleBytes {
                    ref module_bytes, ..
                } => module_bytes.is_empty(),
                _ => false,
            };

            // Create payment code module from bytes
            // validation_spec_1: valid wasm bytes
            let maybe_payment_module = if module_bytes_is_empty {
                let standard_payment_hash: ContractHash =
                    match self.state.get_protocol_data(protocol_version) {
                        Ok(Some(protocol_data)) => protocol_data.standard_payment(),
                        Ok(None) => {
                            return Ok(ExecutionResult::precondition_failure(
                                Error::InvalidProtocolVersion(protocol_version),
                            ));
                        }
                        Err(_) => return Ok(ExecutionResult::precondition_failure(Error::Deploy)),
                    };

                // if "use-system-contracts" is false, "do_nothing" wasm is returned
                self.get_module_from_contract_hash(
                    Rc::clone(&tracking_copy),
                    standard_payment_hash,
                    correlation_id,
                    &protocol_version,
                )
                .map(|module| GetModuleResult::Session {
                    module,
                    contract_package: ContractPackage::default(),
                    entry_point: EntryPoint::default(),
                })
            } else {
                self.get_module(
                    Rc::clone(&tracking_copy),
                    &payment,
                    &account,
                    correlation_id,
                    preprocessor,
                    &protocol_version,
                )
            };

            let payment_module = match maybe_payment_module {
                Ok(module) => module,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error));
                }
            };

            // payment_code_spec_2: execute payment code
            let phase = Phase::Payment;
            let (
                payment_module,
                payment_base_key,
                mut payment_named_keys,
                payment_package,
                payment_entry_point,
            ) = match payment_module {
                GetModuleResult::Session {
                    module,
                    contract_package,
                    entry_point,
                } => (
                    module,
                    base_key,
                    account.named_keys().clone(),
                    contract_package,
                    entry_point,
                ),
                GetModuleResult::Contract {
                    module,
                    base_key,
                    contract,
                    contract_package,
                    entry_point,
                } => (
                    module,
                    base_key,
                    contract.named_keys().clone(),
                    contract_package,
                    entry_point,
                ),
            };

            let payment_args = match payment.into_runtime_args() {
                Ok(args) => args,
                Err(e) => {
                    let exec_err: execution::Error = e.into();
                    warn!("Unable to deserialize arguments: {:?}", exec_err);
                    return Ok(ExecutionResult::precondition_failure(exec_err.into()));
                }
            };

            let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);

            if self.config.use_system_contracts() || !module_bytes_is_empty {
                executor.exec(
                    payment_module,
                    payment_entry_point,
                    payment_args,
                    payment_base_key,
                    &account,
                    &mut payment_named_keys,
                    authorization_keys.clone(),
                    blocktime,
                    deploy_hash,
                    pay_gas_limit,
                    protocol_version,
                    correlation_id,
                    Rc::clone(&tracking_copy),
                    phase,
                    protocol_data,
                    system_contract_cache,
                    &payment_package,
                )
            } else {
                // use host side standard payment
                let hash_address_generator = {
                    let generator = AddressGenerator::new(&deploy_hash, phase);
                    Rc::new(RefCell::new(generator))
                };
                let uref_address_generator = {
                    let generator = AddressGenerator::new(&deploy_hash, phase);
                    Rc::new(RefCell::new(generator))
                };

                let mut runtime = match executor.create_runtime(
                    payment_module,
                    EntryPointType::Session,
                    payment_args,
                    &mut payment_named_keys,
                    Default::default(),
                    payment_base_key,
                    &account,
                    authorization_keys.clone(),
                    blocktime,
                    deploy_hash,
                    pay_gas_limit,
                    hash_address_generator,
                    uref_address_generator,
                    protocol_version,
                    correlation_id,
                    Rc::clone(&tracking_copy),
                    phase,
                    protocol_data,
                    system_contract_cache,
                ) {
                    Ok((_instance, runtime)) => runtime,
                    Err(error) => {
                        return Ok(ExecutionResult::precondition_failure(Error::Exec(error)));
                    }
                };

                let effects_snapshot = tracking_copy.borrow().effect();

                match runtime.call_host_standard_payment() {
                    Ok(()) => ExecutionResult::Success {
                        effect: runtime.context().effect(),
                        cost: runtime.context().gas_counter(),
                    },
                    Err(error) => ExecutionResult::Failure {
                        error: error.into(),
                        effect: effects_snapshot,
                        cost: runtime.context().gas_counter(),
                    },
                }
            }
        };

        debug!("Payment result: {:?}", payment_result);

        let payment_result_cost = payment_result.cost();
        // payment_code_spec_3: fork based upon payment purse balance and cost of
        // payment code execution
        let payment_purse_balance: Motes = {
            // Get payment purse Key from proof of stake contract
            // payment_code_spec_6: system contract validity
            let payment_purse_key: Key =
                match proof_of_stake_contract.named_keys().get(POS_PAYMENT_PURSE) {
                    Some(key) => *key,
                    None => return Ok(ExecutionResult::precondition_failure(Error::Deploy)),
                };

            let purse_balance_key = match tracking_copy
                .borrow_mut()
                .get_purse_balance_key(correlation_id, payment_purse_key)
            {
                Ok(key) => key,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            };

            match tracking_copy
                .borrow_mut()
                .get_purse_balance(correlation_id, purse_balance_key)
            {
                Ok(balance) => balance,
                Err(error) => {
                    return Ok(ExecutionResult::precondition_failure(error.into()));
                }
            }
        };

        if let Some(forced_transfer) = payment_result.check_forced_transfer(payment_purse_balance) {
            // Get rewards purse balance key
            // payment_code_spec_6: system contract validity
            let rewards_purse_balance_key: Key = {
                // Get reward purse Key from proof of stake contract
                // payment_code_spec_6: system contract validity
                let rewards_purse_key: Key =
                    match proof_of_stake_contract.named_keys().get(POS_REWARDS_PURSE) {
                        Some(key) => *key,
                        None => {
                            return Ok(ExecutionResult::precondition_failure(Error::Deploy));
                        }
                    };

                match tracking_copy
                    .borrow_mut()
                    .get_purse_balance_key(correlation_id, rewards_purse_key)
                {
                    Ok(key) => key,
                    Err(error) => {
                        return Ok(ExecutionResult::precondition_failure(error.into()));
                    }
                }
            };

            let error = match forced_transfer {
                ForcedTransferResult::InsufficientPayment => Error::InsufficientPayment,
                ForcedTransferResult::PaymentFailure => payment_result.take_error().unwrap(),
            };
            return Ok(ExecutionResult::new_payment_code_error(
                error,
                max_payment_cost,
                account_main_purse_balance,
                account_main_purse_balance_key,
                rewards_purse_balance_key,
            ));
        }

        execution_result_builder.set_payment_execution_result(payment_result);

        let post_payment_tracking_copy = tracking_copy.borrow();
        let session_tracking_copy = Rc::new(RefCell::new(post_payment_tracking_copy.fork()));

        // session_code_spec_2: execute session code
        let (
            session_module,
            session_base_key,
            mut session_named_keys,
            session_package,
            session_entry_point,
        ) = match session_module {
            GetModuleResult::Session {
                module,
                contract_package,
                entry_point,
            } => (
                module,
                base_key,
                account.named_keys().clone(),
                contract_package,
                entry_point,
            ),
            GetModuleResult::Contract {
                module,
                base_key,
                contract,
                contract_package,
                entry_point,
            } => (
                module,
                base_key,
                contract.named_keys().clone(),
                contract_package,
                entry_point,
            ),
        };

        let session_args = match session.into_runtime_args() {
            Ok(args) => args,
            Err(e) => {
                let exec_err: execution::Error = e.into();
                warn!("Unable to deserialize session arguments: {:?}", exec_err);
                return Ok(ExecutionResult::precondition_failure(exec_err.into()));
            }
        };
        let session_result = {
            // payment_code_spec_3_b_i: if (balance of PoS pay purse) >= (gas spent during
            // payment code execution) * conv_rate, yes session
            // session_code_spec_1: gas limit = ((balance of PoS payment purse) / conv_rate)
            // - (gas spent during payment execution)
            let session_gas_limit: Gas = Gas::from_motes(payment_purse_balance, CONV_RATE)
                .unwrap_or_default()
                - payment_result_cost;
            let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);

            executor.exec(
                session_module,
                session_entry_point,
                session_args,
                session_base_key,
                &account,
                &mut session_named_keys,
                authorization_keys.clone(),
                blocktime,
                deploy_hash,
                session_gas_limit,
                protocol_version,
                correlation_id,
                Rc::clone(&session_tracking_copy),
                Phase::Session,
                protocol_data,
                system_contract_cache,
                &session_package,
            )
        };
        debug!("Session result: {:?}", session_result);

        let post_session_rc = if session_result.is_failure() {
            // If session code fails we do not include its effects,
            // so we start again from the post-payment state.
            Rc::new(RefCell::new(post_payment_tracking_copy.fork()))
        } else {
            session_tracking_copy
        };

        // NOTE: session_code_spec_3: (do not include session execution effects in
        // results) is enforced in execution_result_builder.build()
        execution_result_builder.set_session_execution_result(session_result);

        // payment_code_spec_5: run finalize process
        let (_, finalize_result): (Option<()>, ExecutionResult) = {
            let post_session_tc = post_session_rc.borrow();
            let finalization_tc = Rc::new(RefCell::new(post_session_tc.fork()));

            let proof_of_stake_args = {
                //((gas spent during payment code execution) + (gas spent during session code execution)) * conv_rate
                let finalize_cost_motes: Motes =
                    Motes::from_gas(execution_result_builder.total_cost(), CONV_RATE)
                        .expect("motes overflow");
                const ARG_AMOUNT: &str = "amount";
                const ARG_ACCOUNT_KEY: &str = "account";
                runtime_args! {
                    ARG_AMOUNT => finalize_cost_motes.value(),
                    ARG_ACCOUNT_KEY => account_public_key,
                }
            };

            // The PoS keys may have changed because of effects during payment and/or
            // session, so we need to look them up again from the tracking copy
            let proof_of_stake_contract = match finalization_tc
                .borrow_mut()
                .get_contract(correlation_id, proof_of_stake_hash)
            {
                Ok(info) => info,
                Err(error) => return Ok(ExecutionResult::precondition_failure(error.into())),
            };

            let mut proof_of_stake_keys = proof_of_stake_contract.named_keys().to_owned();

            let gas_limit = Gas::new(U512::from(std::u64::MAX));
            let system_contract_cache = SystemContractCache::clone(&self.system_contract_cache);

            executor.exec_system_contract(
                DirectSystemContractCall::FinalizePayment,
                proof_of_stake_module,
                proof_of_stake_args,
                &mut proof_of_stake_keys,
                Default::default(),
                Key::from(protocol_data.proof_of_stake()),
                &system_account,
                authorization_keys,
                blocktime,
                deploy_hash,
                gas_limit,
                protocol_version,
                correlation_id,
                finalization_tc,
                Phase::FinalizePayment,
                protocol_data,
                system_contract_cache,
            )
        };

        execution_result_builder.set_finalize_execution_result(finalize_result);

        // We panic here to indicate that the builder was not used properly.
        let ret = execution_result_builder
            .build(tracking_copy.borrow().reader(), correlation_id)
            .expect("ExecutionResultBuilder not initialized properly");

        // NOTE: payment_code_spec_5_a is enforced in execution_result_builder.build()
        // payment_code_spec_6: return properly combined set of transforms and
        // appropriate error
        Ok(ret)
    }

    pub fn apply_effect(
        &self,
        correlation_id: CorrelationId,
        pre_state_hash: Blake2bHash,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<CommitResult, Error>
    where
        Error: From<S::Error>,
    {
        match self.state.commit(correlation_id, pre_state_hash, effects)? {
            CommitResult::Success { state_root, .. } => Ok(CommitResult::Success { state_root }),
            commit_result => Ok(commit_result),
        }
    }
}
