# Changelog

All notable changes to this project will be documented in this file.  The format is based on [Keep a Changelog].

[comment]: <> (Added:      new features)
[comment]: <> (Changed:    changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed:    now removed features)
[comment]: <> (Fixed:      any bug fixes)
[comment]: <> (Security:   in case of vulnerabilities)



## [Unreleased] (node 2.0)

## [Unreleased] (node 2.0)

### Added

- Add support for a factory pattern on the host side.
- struct casper_execution_engine::engine_state::engine_config::EngineConfig
- struct casper_execution_engine::engine_state::engine_config::EngineConfigBuilder
- const casper_execution_engine::engine_state::engine_config::DEFAULT_ALLOW_AUCTION_BIDS: bool
- const casper_execution_engine::engine_state::engine_config::DEFAULT_ALLOW_UNRESTRICTED_TRANSFERS: bool
- const casper_execution_engine::engine_state::engine_config::DEFAULT_BALANCE_HOLD_INTERVAL: casper_types::timestamp::TimeDiff
- const casper_execution_engine::engine_state::engine_config::DEFAULT_COMPUTE_REWARDS: bool
- const casper_execution_engine::engine_state::engine_config::DEFAULT_ENABLE_ENTITY: bool
- const casper_execution_engine::engine_state::engine_config::DEFAULT_MAXIMUM_DELEGATION_AMOUNT: u64
- const casper_execution_engine::engine_state::engine_config::DEFAULT_MAX_ASSOCIATED_KEYS: u32
- const casper_execution_engine::engine_state::engine_config::DEFAULT_MAX_DELEGATORS_PER_VALIDATOR: u32
- const casper_execution_engine::engine_state::engine_config::DEFAULT_MAX_QUERY_DEPTH: u64
- const casper_execution_engine::engine_state::engine_config::DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32
- const casper_execution_engine::engine_state::engine_config::DEFAULT_MAX_STORED_VALUE_SIZE: u32
- const casper_execution_engine::engine_state::engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT: u64
- const casper_execution_engine::engine_state::engine_config::DEFAULT_PROTOCOL_VERSION: casper_types::protocol_version::ProtocolVersion
- const casper_execution_engine::engine_state::engine_config::DEFAULT_STRICT_ARGUMENT_CHECKING: bool
- const casper_execution_engine::engine_state::engine_config::DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS: u64
- enum casper_execution_engine::engine_state::Error
- enum casper_execution_engine::engine_state::ExecutableItem
- enum casper_execution_engine::engine_state::InvalidRequest
- enum casper_execution_engine::engine_state::SessionInputData<'a>
- struct casper_execution_engine::engine_state::BlockInfo
- struct casper_execution_engine::engine_state::EngineConfig
- struct casper_execution_engine::engine_state::EngineConfigBuilder
- struct casper_execution_engine::engine_state::ExecutionEngineV1
- struct casper_execution_engine::engine_state::SessionDataDeploy<'a>
- struct casper_execution_engine::engine_state::SessionDataV1<'a>
- struct casper_execution_engine::engine_state::WasmV1Request
- struct casper_execution_engine::engine_state::WasmV1Result
- const casper_execution_engine::engine_state::DEFAULT_MAX_QUERY_DEPTH: u64
- const casper_execution_engine::engine_state::DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32
- const casper_execution_engine::engine_state::MAX_PAYMENT_AMOUNT: u64
- const casper_execution_engine::engine_state::WASMLESS_TRANSFER_FIXED_GAS_PRICE: u8
- static casper_execution_engine::engine_state::MAX_PAYMENT: once_cell::sync::Lazy<casper_types::uint::macro_code::U512>
- enum casper_execution_engine::execution::ExecError
- enum casper_execution_engine::resolvers::error::ResolverError
- trait casper_execution_engine::resolvers::memory_resolver::MemoryResolver
- const casper_execution_engine::runtime::cryptography::DIGEST_LENGTH: usize
- fn casper_execution_engine::runtime::cryptography::blake2b<T: core::convert::AsRef<[u8]>>(data: T) -> [u8; 32]
- fn casper_execution_engine::runtime::cryptography::blake3<T: core::convert::AsRef<[u8]>>(data: T) -> [u8; 32]
- fn casper_execution_engine::runtime::cryptography::sha256<T: core::convert::AsRef<[u8]>>(data: T) -> [u8; 32]
- struct casper_execution_engine::runtime::stack::RuntimeStack
- struct casper_execution_engine::runtime::stack::RuntimeStackOverflow
- type casper_execution_engine::runtime::stack::RuntimeStackFrame = casper_types::system::caller::Caller
- enum casper_execution_engine::runtime::PreprocessingError
- enum casper_execution_engine::runtime::WasmValidationError
- struct casper_execution_engine::runtime::Runtime<'a, R>
- struct casper_execution_engine::runtime::RuntimeStack
- struct casper_execution_engine::runtime::RuntimeStackOverflow
- const casper_execution_engine::runtime::DEFAULT_BR_TABLE_MAX_SIZE: u32
- const casper_execution_engine::runtime::DEFAULT_MAX_GLOBALS: u32
- const casper_execution_engine::runtime::DEFAULT_MAX_PARAMETER_COUNT: u32
- const casper_execution_engine::runtime::DEFAULT_MAX_TABLE_SIZE: u32
- fn casper_execution_engine::runtime::cycles_for_instruction(instruction: &casper_wasm::elements::ops::Instruction) -> u32
- fn casper_execution_engine::runtime::preprocess(wasm_config: casper_types::chainspec::vm_config::wasm_config::WasmConfig, module_bytes: &[u8]) -> core::result::Result<casper_wasm::elements::module::Module, casper_execution_engine::runtime::PreprocessingError>
- type casper_execution_engine::runtime::RuntimeStackFrame = casper_types::system::caller::Caller
- enum casper_execution_engine::runtime_context::AllowInstallUpgrade
- struct casper_execution_engine::runtime_context::RuntimeContext<'a, R>
- const casper_execution_engine::runtime_context::RANDOM_BYTES_COUNT: usize

### Removed

- struct casper_execution_engine::config::Config
- enum casper_execution_engine::core::engine_state::balance::BalanceResult
- struct casper_execution_engine::core::engine_state::balance::BalanceRequest
- struct casper_execution_engine::core::engine_state::chainspec_registry::ChainspecRegistry
- struct casper_execution_engine::core::engine_state::checksum_registry::ChecksumRegistry
- struct casper_execution_engine::core::engine_state::deploy_item::DeployItem
- enum casper_execution_engine::core::engine_state::engine_config::FeeHandling
- enum casper_execution_engine::core::engine_state::engine_config::RefundHandling
- struct casper_execution_engine::core::engine_state::engine_config::EngineConfig
- struct casper_execution_engine::core::engine_state::engine_config::EngineConfigBuilder
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_ALLOW_AUCTION_BIDS: bool
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_ALLOW_UNRESTRICTED_TRANSFERS: bool
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_FEE_HANDLING
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_MAX_ASSOCIATED_KEYS
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_MAX_QUERY_DEPTH
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_MAX_STORED_VALUE_SIZE
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_MINIMUM_BID_AMOUNT
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_REFUND_HANDLING
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_STRICT_ARGUMENT_CHECKING
- const casper_execution_engine::core::engine_state::engine_config::DEFAULT_VESTING_SCHEDULE_LENGTH_MILLIS
- enum casper_execution_engine::core::engine_state::era_validators::GetEraValidatorsError
- struct casper_execution_engine::core::engine_state::era_validators::GetEraValidatorsRequest
- enum casper_execution_engine::core::engine_state::executable_deploy_item::ContractIdentifier
- enum casper_execution_engine::core::engine_state::executable_deploy_item::ContractPackageIdentifier
- enum casper_execution_engine::core::engine_state::executable_deploy_item::DeployKind
- enum casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItem
- enum casper_execution_engine::core::engine_state::executable_deploy_item::ExecutionKind
- struct casper_execution_engine::core::engine_state::executable_deploy_item::ExecutableDeployItemDiscriminantsIter
- struct casper_execution_engine::core::engine_state::execute_request::ExecuteRequest
- struct casper_execution_engine::core::engine_state::execution_effect::ExecutionEffect
- enum casper_execution_engine::core::engine_state::execution_result::ExecutionResult
- enum casper_execution_engine::core::engine_state::execution_result::ForcedTransferResult
- struct casper_execution_engine::core::engine_state::execution_result::ExecutionResultBuilder
- type casper_execution_engine::core::engine_state::execution_result::ExecutionResults = alloc::collections::vec_deque::VecDeque<casper_execution_engine::core::engine_state::execution_result::ExecutionResult>
- enum casper_execution_engine::core::engine_state::genesis::GenesisAccount
- enum casper_execution_engine::core::engine_state::genesis::GenesisError
- struct casper_execution_engine::core::engine_state::genesis::AdministratorAccount
- struct casper_execution_engine::core::engine_state::genesis::ExecConfig
- struct casper_execution_engine::core::engine_state::genesis::ExecConfigBuilder
- struct casper_execution_engine::core::engine_state::genesis::GenesisConfig
- struct casper_execution_engine::core::engine_state::genesis::GenesisSuccess
- struct casper_execution_engine::core::engine_state::genesis::GenesisValidator
- const casper_execution_engine::core::engine_state::genesis::DEFAULT_AUCTION_DELAY: u64
- const casper_execution_engine::core::engine_state::genesis::DEFAULT_GENESIS_TIMESTAMP_MILLIS: u64
- const casper_execution_engine::core::engine_state::genesis::DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS: u64
- const casper_execution_engine::core::engine_state::genesis::DEFAULT_ROUND_SEIGNIORAGE_RATE: num_rational::Ratio<u64>
- const casper_execution_engine::core::engine_state::genesis::DEFAULT_UNBONDING_DELAY: u64
- const casper_execution_engine::core::engine_state::genesis::DEFAULT_VALIDATOR_SLOTS: u32
- enum casper_execution_engine::core::engine_state::get_bids::GetBidsResult
- struct casper_execution_engine::core::engine_state::get_bids::GetBidsRequest
- enum casper_execution_engine::core::engine_state::op::Op
- enum casper_execution_engine::core::engine_state::query::QueryResult
- struct casper_execution_engine::core::engine_state::query::QueryRequest
- struct casper_execution_engine::core::engine_state::run_genesis_request::RunGenesisRequest
- enum casper_execution_engine::core::engine_state::step::StepError
- struct casper_execution_engine::core::engine_state::step::EvictItem
- struct casper_execution_engine::core::engine_state::step::RewardItem
- struct casper_execution_engine::core::engine_state::step::SlashItem
- struct casper_execution_engine::core::engine_state::step::StepRequest
- struct casper_execution_engine::core::engine_state::step::StepSuccess
- struct casper_execution_engine::core::engine_state::system_contract_registry::SystemContractRegistry
- enum casper_execution_engine::core::engine_state::upgrade::ProtocolUpgradeError
- struct casper_execution_engine::core::engine_state::upgrade::UpgradeConfig
- struct casper_execution_engine::core::engine_state::upgrade::UpgradeSuccess
- enum casper_execution_engine::core::engine_state::BalanceResult
- enum casper_execution_engine::core::engine_state::Error
- enum casper_execution_engine::core::engine_state::ExecError
- enum casper_execution_engine::core::engine_state::ExecutableDeployItem
- enum casper_execution_engine::core::engine_state::ExecutionResult
- enum casper_execution_engine::core::engine_state::ForcedTransferResult
- enum casper_execution_engine::core::engine_state::GenesisAccount
- enum casper_execution_engine::core::engine_state::GetBidsResult
- enum casper_execution_engine::core::engine_state::GetEraValidatorsError
- enum casper_execution_engine::core::engine_state::PruneResult
- enum casper_execution_engine::core::engine_state::QueryResult
- enum casper_execution_engine::core::engine_state::StepError
- enum casper_execution_engine::core::engine_state::TransferTargetMode
- struct casper_execution_engine::core::engine_state::BalanceRequest
- struct casper_execution_engine::core::engine_state::ChainspecRegistry
- struct casper_execution_engine::core::engine_state::ChecksumRegistry
- struct casper_execution_engine::core::engine_state::DeployItem
- struct casper_execution_engine::core::engine_state::EngineConfig
- struct casper_execution_engine::core::engine_state::EngineConfigBuilder
- struct casper_execution_engine::core::engine_state::EngineState<S>
- struct casper_execution_engine::core::engine_state::ExecConfig
- struct casper_execution_engine::core::engine_state::ExecuteRequest
- struct casper_execution_engine::core::engine_state::GenesisConfig
- struct casper_execution_engine::core::engine_state::GenesisSuccess
- struct casper_execution_engine::core::engine_state::GetBidsRequest
- struct casper_execution_engine::core::engine_state::GetEraValidatorsRequest
- struct casper_execution_engine::core::engine_state::PruneConfig
- struct casper_execution_engine::core::engine_state::QueryRequest
- struct casper_execution_engine::core::engine_state::RewardItem
- struct casper_execution_engine::core::engine_state::RunGenesisRequest
- struct casper_execution_engine::core::engine_state::SlashItem
- struct casper_execution_engine::core::engine_state::StepRequest
- struct casper_execution_engine::core::engine_state::StepSuccess
- struct casper_execution_engine::core::engine_state::SystemContractRegistry
- struct casper_execution_engine::core::engine_state::TransferArgs
- struct casper_execution_engine::core::engine_state::TransferRuntimeArgsBuilder
- struct casper_execution_engine::core::engine_state::UpgradeConfig
- struct casper_execution_engine::core::engine_state::UpgradeSuccess
- const casper_execution_engine::core::engine_state::DEFAULT_MAX_QUERY_DEPTH: u64
- const casper_execution_engine::core::engine_state::DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT: u32
- const casper_execution_engine::core::engine_state::MAX_PAYMENT_AMOUNT: u64
- const casper_execution_engine::core::engine_state::WASMLESS_TRANSFER_FIXED_GAS_PRICE: u64
- static casper_execution_engine::core::engine_state::MAX_PAYMENT: once_cell::sync::Lazy<casper_types::uint::macro_code::U512>
- enum casper_execution_engine::core::execution::Error
- enum casper_execution_engine::core::resolvers::error::ResolverError
- trait casper_execution_engine::core::resolvers::memory_resolver::MemoryResolver
- struct casper_execution_engine::core::runtime::stack::RuntimeStack
- struct casper_execution_engine::core::runtime::stack::RuntimeStackOverflow
- type casper_execution_engine::core::runtime::stack::RuntimeStackFrame = casper_types::system::call_stack_element::CallStackElement
- struct casper_execution_engine::core::runtime::Runtime<'a, R>
- struct casper_execution_engine::core::runtime_context::RuntimeContext<'a, R>
- const casper_execution_engine::core::runtime_context::RANDOM_BYTES_COUNT: usize
- fn casper_execution_engine::core::runtime_context::validate_group_membership(contract_package: &casper_types::contracts::ContractPackage, access: &casper_types::contracts::EntryPointAccess, validator: impl core::ops::function::Fn(&casper_types::uref::URef) -> bool) -> core::result::Result<(), casper_execution_engine::core::engine_state::ExecError>
- enum casper_execution_engine::core::tracking_copy::AddResult
- enum casper_execution_engine::core::tracking_copy::TrackingCopyQueryResult
- enum casper_execution_engine::core::tracking_copy::ValidationError
- struct casper_execution_engine::core::tracking_copy::TrackingCopy<R>
- struct casper_execution_engine::core::tracking_copy::TrackingCopyCache<M>
- trait casper_execution_engine::core::tracking_copy::TrackingCopyExt<R>
- fn casper_execution_engine::core::tracking_copy::validate_balance_proof(hash: &casper_hashing::Digest, balance_proof: &casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProof<casper_types::key::Key, casper_types::stored_value::StoredValue>, expected_purse_key: casper_types::key::Key, expected_motes: &casper_types::uint::macro_code::U512) -> core::result::Result<(), casper_execution_engine::core::tracking_copy::ValidationError>
- fn casper_execution_engine::core::tracking_copy::validate_query_proof(hash: &casper_hashing::Digest, proofs: &[casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProof<casper_types::key::Key, casper_types::stored_value::StoredValue>], expected_first_key: &casper_types::key::Key, path: &[alloc::string::String], expected_value: &casper_types::stored_value::StoredValue) -> core::result::Result<(), casper_execution_engine::core::tracking_copy::ValidationError>
- enum casper_execution_engine::core::ValidationError
- const casper_execution_engine::core::ADDRESS_LENGTH: usize
- fn casper_execution_engine::core::validate_balance_proof(hash: &casper_hashing::Digest, balance_proof: &casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProof<casper_types::key::Key, casper_types::stored_value::StoredValue>, expected_purse_key: casper_types::key::Key, expected_motes: &casper_types::uint::macro_code::U512) -> core::result::Result<(), casper_execution_engine::core::tracking_copy::ValidationError>
- fn casper_execution_engine::core::validate_query_proof(hash: &casper_hashing::Digest, proofs: &[casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProof<casper_types::key::Key, casper_types::stored_value::StoredValue>], expected_first_key: &casper_types::key::Key, path: &[alloc::string::String], expected_value: &casper_types::stored_value::StoredValue) -> core::result::Result<(), casper_execution_engine::core::tracking_copy::ValidationError>
- type casper_execution_engine::core::Address = [u8; 32]
- struct casper_execution_engine::shared::additive_map::AdditiveMap<K, V, S>
- struct casper_execution_engine::shared::execution_journal::ExecutionJournal
- struct casper_execution_engine::shared::host_function_costs::HostFunction<T>
- struct casper_execution_engine::shared::host_function_costs::HostFunctionCosts
- type casper_execution_engine::shared::host_function_costs::Cost = u32
- enum casper_execution_engine::shared::logging::Style
- struct casper_execution_engine::shared::logging::Settings
- fn casper_execution_engine::shared::logging::initialize(settings: casper_execution_engine::shared::logging::Settings) -> core::result::Result<(), log::SetLoggerError>
- fn casper_execution_engine::shared::logging::log_details(\_log_level: log::Level, \_message_format: alloc::string::String, \_properties: alloc::collections::btree::map::BTreeMap<&str, alloc::string::String>)
- fn casper_execution_engine::shared::logging::log_host_function_metrics(\_host_function: &str, \_properties: alloc::collections::btree::map::BTreeMap<&str, alloc::string::String>)
- struct casper_execution_engine::shared::newtypes::CorrelationId
- struct casper_execution_engine::shared::opcode_costs::BrTableCost
- struct casper_execution_engine::shared::opcode_costs::ControlFlowCosts
- struct casper_execution_engine::shared::opcode_costs::OpcodeCosts
- const casper_execution_engine::shared::opcode_costs::DEFAULT_ADD_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_BIT_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONST_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_BLOCK_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_BR_IF_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_BR_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_BR_TABLE_MULTIPLIER: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_BR_TABLE_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_CALL_INDIRECT_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_CALL_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_DROP_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_ELSE_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_END_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_IF_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_LOOP_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_RETURN_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONTROL_FLOW_SELECT_OPCODE: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CONVERSION_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_CURRENT_MEMORY_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_DIV_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_GLOBAL_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_GROW_MEMORY_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_INTEGER_COMPARISON_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_LOAD_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_LOCAL_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_MUL_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_NOP_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_STORE_COST: u32
- const casper_execution_engine::shared::opcode_costs::DEFAULT_UNREACHABLE_COST: u32
- struct casper_execution_engine::shared::storage_costs::StorageCosts
- const casper_execution_engine::shared::storage_costs::DEFAULT_GAS_PER_BYTE_COST: u32
- struct casper_execution_engine::shared::system_config::auction_costs::AuctionCosts
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_ACTIVATE_BID_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_ADD_BID_COST: u64
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_DELEGATE_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_DISTRIBUTE_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_GET_ERA_VALIDATORS_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_READ_ERA_ID_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_READ_SEIGNIORAGE_RECIPIENTS_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_REDELEGATE_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_RUN_AUCTION_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_SLASH_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_UNDELEGATE_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_WITHDRAW_BID_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_WITHDRAW_DELEGATOR_REWARD_COST: u32
- const casper_execution_engine::shared::system_config::auction_costs::DEFAULT_WITHDRAW_VALIDATOR_REWARD_COST: u32
- struct casper_execution_engine::shared::system_config::handle_payment_costs::HandlePaymentCosts
- const casper_execution_engine::shared::system_config::handle_payment_costs::DEFAULT_FINALIZE_PAYMENT_COST: u32
- const casper_execution_engine::shared::system_config::handle_payment_costs::DEFAULT_GET_PAYMENT_PURSE_COST: u32
- const casper_execution_engine::shared::system_config::handle_payment_costs::DEFAULT_GET_REFUND_PURSE_COST: u32
- const casper_execution_engine::shared::system_config::handle_payment_costs::DEFAULT_SET_REFUND_PURSE_COST: u32
- struct casper_execution_engine::shared::system_config::mint_costs::MintCosts
- const casper_execution_engine::shared::system_config::mint_costs::DEFAULT_BALANCE_COST: u32
- const casper_execution_engine::shared::system_config::mint_costs::DEFAULT_CREATE_COST: u32
- const casper_execution_engine::shared::system_config::mint_costs::DEFAULT_MINT_COST: u32
- const casper_execution_engine::shared::system_config::mint_costs::DEFAULT_MINT_INTO_EXISTING_PURSE_COST: u32
- const casper_execution_engine::shared::system_config::mint_costs::DEFAULT_READ_BASE_ROUND_REWARD_COST: u32
- const casper_execution_engine::shared::system_config::mint_costs::DEFAULT_REDUCE_TOTAL_SUPPLY_COST: u32
- const casper_execution_engine::shared::system_config::mint_costs::DEFAULT_TRANSFER_COST: u32
- struct casper_execution_engine::shared::system_config::standard_payment_costs::StandardPaymentCosts
- struct casper_execution_engine::shared::system_config::SystemConfig
- const casper_execution_engine::shared::system_config::DEFAULT_WASMLESS_TRANSFER_COST: u32
- fn casper_execution_engine::shared::test_utils::mocked_account(account_hash: casper_types::account::account_hash::AccountHash) -> alloc::vec::Vec<(casper_types::key::Key, casper_types::stored_value::StoredValue)>
- enum casper_execution_engine::shared::transform::Error
- enum casper_execution_engine::shared::transform::Transform
- static casper_execution_engine::shared::utils::OS_PAGE_SIZE: once_cell::sync::Lazy<usize>
- fn casper_execution_engine::shared::utils::check_multiple_of_page_size(value: usize)
- fn casper_execution_engine::shared::utils::jsonify<T>(value: T, pretty_print: bool) -> alloc::string::String where T: serde::ser::Serialize
- struct casper_execution_engine::shared::wasm_config::WasmConfig
- const casper_execution_engine::shared::wasm_config::DEFAULT_MAX_STACK_HEIGHT: u32
- const casper_execution_engine::shared::wasm_config::DEFAULT_WASM_MAX_MEMORY: u32
- enum casper_execution_engine::shared::wasm_prep::PreprocessingError
- enum casper_execution_engine::shared::wasm_prep::WasmValidationError
- const casper_execution_engine::shared::wasm_prep::DEFAULT_BR_TABLE_MAX_SIZE: u32
- const casper_execution_engine::shared::wasm_prep::DEFAULT_MAX_GLOBALS: u32
- const casper_execution_engine::shared::wasm_prep::DEFAULT_MAX_PARAMETER_COUNT: u32
- const casper_execution_engine::shared::wasm_prep::DEFAULT_MAX_TABLE_SIZE: u32
- fn casper_execution_engine::shared::wasm_prep::deserialize(module_bytes: &[u8]) -> core::result::Result<casper_wasm::elements::module::Module, casper_execution_engine::shared::wasm_prep::PreprocessingError>
- fn casper_execution_engine::shared::wasm_prep::get_module_from_entry_points(entry_point_names: alloc::vec::Vec<&str>, module: casper_wasm::elements::module::Module) -> core::result::Result<alloc::vec::Vec<u8>, casper_execution_engine::core::engine_state::ExecError>
- fn casper_execution_engine::shared::wasm_prep::preprocess(wasm_config: casper_execution_engine::shared::wasm_config::WasmConfig, module_bytes: &[u8]) -> core::result::Result<casper_wasm::elements::module::Module, casper_execution_engine::shared::wasm_prep::PreprocessingError>
- enum casper_execution_engine::storage::error::in_memory::Error
- enum casper_execution_engine::storage::error::lmdb::Error
- enum casper_execution_engine::storage::error::Error
- struct casper_execution_engine::storage::global_state::in_memory::InMemoryGlobalState
- struct casper_execution_engine::storage::global_state::lmdb::LmdbGlobalState
- struct casper_execution_engine::storage::global_state::scratch::ScratchGlobalState
- enum casper_execution_engine::storage::global_state::CommitError
- trait casper_execution_engine::storage::global_state::CommitProvider: casper_execution_engine::storage::global_state::StateProvider
- trait casper_execution_engine::storage::global_state::StateProvider
- trait casper_execution_engine::storage::global_state::StateReader<K, V>
- fn casper_execution_engine::storage::global_state::commit<'a, R, S, H, E>(environment: &'a R, store: &S, correlation_id: casper_execution_engine::shared::newtypes::CorrelationId, prestate_hash: casper_hashing::Digest, effects: casper_execution_engine::shared::additive_map::AdditiveMap<casper_types::key::Key, casper_execution_engine::shared::transform::Transform, H>) -> core::result::Result<casper_hashing::Digest, E> where R: casper_execution_engine::storage::transaction_source::TransactionSource<'a, Handle = <S as casper_execution_engine::storage::store::Store>::Handle>, S: casper_execution_engine::storage::trie_store::TrieStore<casper_types::key::Key, casper_types::stored_value::StoredValue>, <S as casper_execution_engine::storage::store::Store>::Error: core::convert::From<<R as casper_execution_engine::storage::transaction_source::TransactionSource>::Error>, E: core::convert::From<<R as casper_execution_engine::storage::transaction_source::TransactionSource>::Error> + core::convert::From<<S as casper_execution_engine::storage::store::Store>::Error> + core::convert::From<casper_types::bytesrepr::Error> + core::convert::From<casper_execution_engine::storage::global_state::CommitError>, H: core::hash::BuildHasher
- fn casper_execution_engine::storage::global_state::put_stored_values<'a, R, S, E>(environment: &'a R, store: &S, correlation_id: casper_execution_engine::shared::newtypes::CorrelationId, prestate_hash: casper_hashing::Digest, stored_values: std::collections::hash::map::HashMap<casper_types::key::Key, casper_types::stored_value::StoredValue>) -> core::result::Result<casper_hashing::Digest, E> where R: casper_execution_engine::storage::transaction_source::TransactionSource<'a, Handle = <S as casper_execution_engine::storage::store::Store>::Handle>, S: casper_execution_engine::storage::trie_store::TrieStore<casper_types::key::Key, casper_types::stored_value::StoredValue>, <S as casper_execution_engine::storage::store::Store>::Error: core::convert::From<<R as casper_execution_engine::storage::transaction_source::TransactionSource>::Error>, E: core::convert::From<<R as casper_execution_engine::storage::transaction_source::TransactionSource>::Error> + core::convert::From<<S as casper_execution_engine::storage::store::Store>::Error> + core::convert::From<casper_types::bytesrepr::Error> + core::convert::From<casper_execution_engine::storage::global_state::CommitError>
- trait casper_execution_engine::storage::store::Store<K, V>
- trait casper_execution_engine::storage::store::StoreExt<K, V>: casper_execution_engine::storage::store::Store<K, V>
- struct casper_execution_engine::storage::transaction_source::in_memory::InMemoryEnvironment
- struct casper_execution_engine::storage::transaction_source::in_memory::InMemoryReadTransaction
- struct casper_execution_engine::storage::transaction_source::in_memory::InMemoryReadWriteTransaction<'a>
- struct casper_execution_engine::storage::transaction_source::lmdb::LmdbEnvironment
- trait casper_execution_engine::storage::transaction_source::Readable: casper_execution_engine::storage::transaction_source::Transaction
- trait casper_execution_engine::storage::transaction_source::Transaction: core::marker::Sized
- trait casper_execution_engine::storage::transaction_source::TransactionSource<'a>
- trait casper_execution_engine::storage::transaction_source::Writable: casper_execution_engine::storage::transaction_source::Transaction
- fn casper_execution_engine::storage::transaction_source::Writable::write(&mut self, handle: Self::Handle, key: &[u8], value: &[u8]) -> core::result::Result<(), Self::Error>
- impl<'a> casper_execution_engine::storage::transaction_source::Writable for lmdb::transaction::RwTransaction<'a>
- fn lmdb::transaction::RwTransaction<'a>::write(&mut self, handle: Self::Handle, key: &[u8], value: &[u8]) -> core::result::Result<(), Self::Error>
- enum casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProofStep
- struct casper_execution_engine::storage::trie::merkle_proof::TrieMerkleProof<K, V>
- enum casper_execution_engine::storage::trie::DescendantsIterator<'a>
- enum casper_execution_engine::storage::trie::Pointer
- enum casper_execution_engine::storage::trie::Trie<K, V>
- struct casper_execution_engine::storage::trie::PointerBlock
- struct casper_execution_engine::storage::trie::TrieRaw
- type casper_execution_engine::storage::trie::Parents<K, V> = alloc::vec::Vec<(u8, casper_execution_engine::storage::trie::Trie<K, V>)>
- type casper_execution_engine::storage::trie::PointerBlockArray = [casper_execution_engine::storage::trie::PointerBlockValue; 256]
- type casper_execution_engine::storage::trie::PointerBlockValue = core::option::Option<casper_execution_engine::storage::trie::Pointer>
- struct casper_execution_engine::storage::trie_store::in_memory::InMemoryTrieStore
- struct casper_execution_engine::storage::trie_store::lmdb::LmdbTrieStore
- trait casper_execution_engine::storage::trie_store::TrieStore<K, V>: casper_execution_engine::storage::store::Store<casper_hashing::Digest, casper_execution_engine::storage::trie::Trie<K, V>>
- macro casper_execution_engine::make_array_newtype!


## 7.0.1

### Changed
* Change the cost of `wasm.storage_costs.gas_per_byte` and `shared::storage_costs::DEFAULT_GAS_PER_BYTE_COST` from `630_000` to `1_117_587`.
* Change the cost of the host function `casper_add_associated_key` from `9_000` to `1_200_000`.
* Change the cost of the argument `entry_points_size` of host function `casper_add_contract_version` from `0` to `120_000`.
* Change the cost of the host function `casper_blake2b`and its argument `in_size` from `200` and `0` respectively to `1_200_000` to `120_000`.
* Change the cost of the host function `casper_call_contract` and its arguments `entry_point_name_size` and `runtime_args_size` from `4_500`, `0` and `420` respectively to `300_000_000`, `120_000` and `120_000`.
* Change the cost of the host function `casper_call_versioned_contract` and the arguments `entry_point_name_size` and `runtime_args_size` from `4_500`, `0` and `420` respectively to `300_000_000`, `120_000` and `120_000`.
* Change the cost of the host function `casper_get_balance` from `3_800` to `3_000_000`.
* Change the cost of arguments `name_size` and `dest_size` of host function `casper_get_named_arg` from `0` to `120_000`.
* Change the cost of the host function `casper_put_key` and its arguments `name_size` and `key_size` from `38_000`, `1_100` and `0` respectively to `100_000_000`, `120_000` and `120_000`.
* Change the cost of the host function `casper_read_value` and its argument `key_size` from `6_000` and `0` respectively to `60_000` and `120_000`.
* Change the cost of the argument `urefs_size` of host function `casper_remove_contract_user_group_urefs` from `0` to `120_000`.
* Change the cost of the host function `casper_transfer_from_purse_to_purse` from `82_000` to `82_000_000`.



## [Unreleased] (node 1.5.4)
## 7.0.0

### Added
* Add chainspec option `core.allow_unrestricted_transfers` that, if enabled, allows token transfers between any two peers. Disabling this option makes sense only for private chains.
* Add chainspec option `core.allow_auction_bids` that, if enabled, allows auction entrypoints `delegate` and `add_bid` to operate. Disabling this option makes sense only for private chains.
* Add chainspec option `core.compute_rewards` that, if enabled, computes rewards for each era. Disabling this option makes sense only for private chains.
* Add chainspec option `core.refund_handling` that specifies how payment refunds are handled.
* Add chainspec option `core.fee_handling` that specifies how transaction fees are handled.
* Add chainspec option `core.administrators` that, if set, contains list of administrator accounts. This option makes sense only for private chains.
* Add support for a new FFI function `enable_contract_version` for enabling a specific version of a contract.

### Changed
* `current stack height` is written to `stderr` in case `Trap(Unreachable)` error is encountered during Wasm execution.
* Tweak upgrade logic transforming withdraw purses to early exit if possible.
* Lower the default gas costs of opcodes.
  - Set the cost for branching opcodes to 35,000 (`br`, `br_if`, `br_table`).
  - Set the cost for call opcodes to 68,000 (`call`, `call_indirect`).
* Default value for round seigniorage rate is halved to `7/175070816` due to reduction in block times, to maintain current seigniorage rate (per unit of time).
* Refund ratio is changed from 0% to 99%.



## 6.0.0

### Changed
* Default value for `max_stack_height` is increased to 500.
* Replace usage of `parity-wasm` and `wasmi` with Casper forks `casper-wasm` and `casper-wasmi` respectively.

### Fixed
* Fix incorrect handling of unbonding purses for validators that were also evicted in that era.
* Fix issue with one-time code used for migrating data to support redelegations.

### Security
* Fix unbounded memory allocation issue while parsing Wasm.



## 5.0.0

### Added
* Add a new entry point `redelegate` to the Auction system contract which allows users to redelegate to another validator without having to unbond. The function signature for the entrypoint is: `redelegate(delegator: PublicKey, validator: PublicKey, amount: U512, new_validator: PublicKey)`
* Add a new type `ChainspecRegistry` which contains the hashes of the `chainspec.toml` and will optionally contain the hashes for `accounts.toml` and `global_state.toml`.
* Add ability to enable strict args checking when executing a contract; i.e. that all non-optional args are provided and of the correct `CLType`.

### Changed
* Fix some integer casts.
* Change both genesis and upgrade functions to write `ChainspecRegistry` under the fixed `Key::ChainspecRegistry`.
* Lift the temporary limit of the size of individual values stored in global state.
* Providing incorrect Wasm for execution will cause the default 2.5CSPR to be charged.
* Update the default `control_flow` opcode cost from `440` to `440_000`.



## 4.0.0

### Changed
* Update dependencies (in particular `casper-types` to v2.0.0 due to additional `Key` variant, requiring a major version bump here).



## 3.1.1

### Changed
* Update the following constant values to match settings in production chainspec:
  * `DEFAULT_RET_VALUE_SIZE_WEIGHT`
  * `DEFAULT_CONTROL_FLOW_CALL_OPCODE`
  * `DEFAULT_CONTROL_FLOW_CALL_INDIRECT_OPCODE`
  * `DEFAULT_GAS_PER_BYTE_COST`
  * `DEFAULT_ADD_BID_COST`
  * `DEFAULT_WITHDRAW_BID_COST`
  * `DEFAULT_DELEGATE_COST`
  * `DEFAULT_UNDELEGATE_COST`
  * `DEFAULT_MAX_STACK_HEIGHT`



## 3.1.0

### Added
* Add `commit_prune` functionality to support pruning of entries in global storage.

### Changed
* Update to use `casper-wasm-utils`; a patched fork of the archived `wasm-utils`.



## 3.0.0

### Changed
* Implement more precise control over opcode costs that lowers the gas cost.
* Increase cost of `withdraw_bid` and `undelegate` auction entry points to 2.5CSPR.



## 2.0.1

### Security
* Implement checks before preprocessing Wasm to avoid potential OOM when initializing table section.
* Implement checks before preprocessing Wasm to avoid references to undeclared functions or globals.
* Implement checks before preprocessing Wasm to avoid possibility to import internal host functions.


## 2.0.0 - 2022-05-11

### Changed
* Change contract runtime to allow caching global state changes during execution of a single block, also avoiding writing interstitial data to global state.



## 1.5.0 - 2022-04-05

### Changed
* Temporarily limit the size of individual values stored in global state.

### Security
* `amount` argument is now required for transactions wanting to send tokens using account's main purse. It is now an upper limit on all tokens being transferred within the transaction.
* Significant rework around the responsibilities of the executor, runtime and runtime context objects, with a focus on removing alternate execution paths where unintended escalation of privilege was possible.
* Attenuate the main purse URef to remove WRITE permissions by default when returned via `ret` or passed as a runtime argument.
* Fix a potential panic during Wasm preprocessing.
* `get_era_validators` performs a query rather than execution.



## 1.4.4 - 2021-12-29

### Changed
* No longer checksum-hex encode hash digest and address types.



## 1.4.3 - 2021-12-06

### Changed
* Auction contract now handles minting into an existing purse.
* Default maximum stack size in `WasmConfig` changed to 188.
* Default behavior of LMDB changed to use [`NO_READAHEAD`](https://docs.rs/lmdb/0.8.0/lmdb/struct.EnvironmentFlags.html#associatedconstant.NO_READAHEAD)

### Fixed
* Fix a case where an unlocked and partially unbonded genesis validator with smaller stake incorrectly occupies slot for a non-genesis validator with higher stake.



## [1.4.2] - 2021-11-11

### Changed
* Execution transforms are returned in their insertion order.

### Removed
* Removed `SystemContractCache` as it was not being used anymore

## [1.4.0] - 2021-10-04

### Added
* Added genesis validation step to ensure there are more genesis validators than validator slots.
* Added a support for passing a public key as a `target` argument in native transfers.
* Added a `max_associated_keys` configuration option for a hard limit of associated keys under accounts.

### Changed
* Documented `storage` module and children.
* Reduced visibility to `pub(crate)` in several areas, allowing some dead code to be noticed and pruned.
* Support building and testing using stable Rust.
* Increase price of `create_purse` to 2.5CSPR.
* Increase price of native transfer to 100 million motes (0.1 CSPR).
* Improve doc comments to clarify behavior of the bidding functionality.
* Document `core` and `shared` modules and their children.
* Change parameters to `LmdbEnvironment`'s constructor enabling manual flushing to disk.

### Fixed
* Fix a case where user could potentially supply a refund purse as a payment purse.



## [1.3.0] - 2021-07-19

### Changed
* Update pinned version of Rust to `nightly-2021-06-17`.



## [1.2.0] - 2021-05-27

### Added
* Add validation that the delegated amount of each genesis account is non-zero.
* Add `activate-bid` client contract.
* Add a check in `Mint::transfer` that the source has `Read` permissions.

### Changed
* Change to Apache 2.0 license.
* Remove the strict expectation that minor and patch protocol versions must always increase by 1.

### Removed
* Remove `RootNotFound` error struct.



## [1.1.1] - 2021-04-19

No changes.



## [1.1.0] - 2021-04-13 [YANKED]

No changes.



## [1.0.1] - 2021-04-08

No changes.



## [1.0.0] - 2021-03-30

### Added
* Initial release of execution engine for Casper mainnet.



[Keep a Changelog]: https://keepachangelog.com/en/1.0.0
[unreleased]: https://github.com/casper-network/casper-node/compare/37d561634adf73dab40fffa7f1f1ee47e80bf8a1...dev
[1.4.2]: https://github.com/casper-network/casper-node/compare/v1.4.0...37d561634adf73dab40fffa7f1f1ee47e80bf8a1
[1.4.0]: https://github.com/casper-network/casper-node/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/casper-network/casper-node/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/casper-network/casper-node/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.1.0]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.0.1]: https://github.com/casper-network/casper-node/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/casper-network/casper-node/releases/tag/v1.0.0
