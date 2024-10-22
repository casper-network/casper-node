# Changelog

All notable changes to this project will be documented in this file. The format is based on [Keep a Changelog].

[comment]: <> (Added: new features)
[comment]: <> (Changed: changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed: now removed features)
[comment]: <> (Fixed: any bug fixes)
[comment]: <> (Security: in case of vulnerabilities)

## [Unreleased] (node 2.0)

### Added

- enum EntityKind
- enum addressable_entity::EntityKindTag
- enum EntityAddr
- struct addressable_entity::NamedKeyAddr
- struct addressable_entity::NamedKeyValue
- struct addressable_entity::MessageTopics
- enum addressable_entity::MessageTopicError
- struct AddressableEntity
- struct addressable_entity::ActionThresholds
- enum addressable_entity::ActionType
- struct addressable_entity::AssociatedKeys
- struct contract::EntryPoint
- enum EntryPointType
- enum EntryPointPayment
- struct EntryPoint
- enum EntryPointAddr
- enum EntryPointValue
- enum addressable_entity::FromAccountHashStrError
- enum addressable_entity::SetThresholdFailure
- struct addressable_entity::TryFromSliceForAccountHashError
- struct addressable_entity::NamedKeys
- struct BlockV1
- struct BlockBodyV1
- struct BlockV2
- struct BlockHeaderV2
- struct BlockBodyV2
- struct ChainNameDigest
- enum EraEnd
- struct EraEndV1
- struct EraEndV2
- struct EraReport
- enum FinalitySignature
- struct FinalitySignatureV1
- struct FinalitySignatureV2
- struct FinalitySignatureId
- struct JsonBlockWithSignatures
- struct RewardedSignatures
- struct SingleBlockRewardedSignatures
- enum Rewards
- struct SignedBlock
- enum SignedBlockHeaderValidationError
- struct SignedBlockHeader
- enum BlockValidationError (moved from casper-node)
- enum Block (don't confuse with previous `Block` struct, see `Changed` section for details)
- enum BlockHeader (don't confuse with previous `BlockHeader` struct, see `Changed` section for details)
- struct HoldsEpoch
- struct addressable_entity::TryFromSliceForContractHashError
- enum addressable_entity::FromStrError
- enum contract_messages::FromStrError
- enum ByteCodeAddr
- struct ByteCodeHash
- enum ByteCodeKind
- struct ByteCode
- struct Chainspec
- struct AccountsConfig
- struct AccountConfig
- struct DelegatorConfig
- struct GenesisValidator
- struct AdministratorAccount
- enum GenesisAccount
- struct ValidatorConfig
- enum ActivationPoint
- struct ChainspecRawBytes
- struct CoreConfig
- enum ConsensusProtocolName
- enum LegacyRequiredFinality
- enum FeeHandling
- struct GenesisConfig
- struct GenesisConfigBuilder
- struct GlobalStateUpdateConfig
- struct GlobalStateUpdate
- enum GlobalStateUpdateError
- struct HighwayConfig
- enum HoldBalanceHandling
- struct NetworkConfig
- struct NextUpgrade
- enum PricingHandling
- struct ProtocolConfig
- enum RefundHandling
- struct TransactionConfig
- struct TransactionLimitsDefinition
- struct TransactionV1Config
- struct ProtocolUpgradeConfig
- struct VacancyConfig
- struct AuctionCosts
- struct ChainspecRegistry
- struct HandlePaymentCosts
- struct HostFunctionCosts
- struct MessageLimits
- struct MintCosts
- struct BrTableCost
- struct ControlFlowCosts
- struct OpcodeCosts
- struct StandardPaymentCosts
- struct StorageCosts
- struct SystemConfig
- struct WasmConfig
- struct WasmV1Config
- struct ChecksumRegistry
- struct SystemEntityRegistry
- struct contract_messages::MessageAddr
- type contract_messages::Messages
- struct contract_messages::MessageChecksum
- enum contract_messages::MessagePayload
- struct contract_messages::Message
- struct contract_messages::TopicNameHash
- struct contract_messages::MessageTopicSummary
- struct Contract
- struct EntryPoints
- struct Digest
- struct DigestError
- struct ChunkWithProof
- enum MerkleConstructionError
- enum MerkleVerificationError
- struct IndexedMerkleProof
- struct DisplayIter
- struct execution::Effects;
- enum execution::ExecutionResult (not to be confused with previous `ExecutionResult`, see `Changed` secion for details)
- struct execution::ExecutionResultV2
- struct execution::TransformV2
- struct execution::TransformError
- struct execution::TransformInstruction
- struct execution::TransformKindV2
- struct execution::PaymentInfo
- enum global_state::TrieMerkleProofStep
- enum global_state::TrieMerkleProof
- struct Pointer
- trait GasLimited
- enum AddressableEntityIdentifier
- struct Approval
- struct ApprovalsHash
- enum InvalidDeploy
- enum DeployBuilder
- enum DeployBuilderError
- enum DeployDecodeFromJsonError
- struct ExecutableDeployItem,
- enum ExecutableDeployItemIdentifier
- struct ExecutionInfo
- enum InitiatorAddr,
- enum InvalidTransaction,
- enum InvalidTransactionV1
- enum PackageIdentifier
- enum PricingMode
- enum PricingModeError
- enum Transaction
- enum TransactionEntryPoint,
- enum TransactionHash
- struct TransactionId
- enum TransactionInvocationTarget
- enum TransactionRuntime
- enum TransactionScheduling
- enum TransactionTarget
- struct TransactionV1,
- struct TransactionV1Payload,
- struct TransactionV1Hash
- struct TransactionV1Builder
- enum TransactionV1BuilderError
- enum TransactionV1DecodeFromJsonError
- enum TransactionV1Error
- struct TransactionV1ExcessiveSizeError
- enum TransferTarget
- struct TransferV2
- enum ValidatorChange
- type contracts::ProtocolVersionMajor
- type EntityVersion
- struct EntityVersionKey
- struct EntityVersions
- struct PackageHash
- enum PackageStatus
- struct Package
- struct PeerEntry
- struct Peers
- enum system::auction::BidAddr
- enum system::auction::BidAddrTag
- enum system::auction::BidKind
- enum system::auction::BidKindTag
- enum system::auction::Bridge
- enum system::auction::Reservation
- enum system::auction::ValidatorBid;
- enum system::auction::ValidatorBids
- enum system::auction::DelegatorBids
- enum system::auction::ValidatorCredits
- enum system::auction::Staking
- trait system::auction::BidsExt
- enum system::auction::Error has new variants: ForgedReference, MissingPurse, ValidatorBidExistsAlready,BridgeRecordChainTooLong,UnexpectedBidVariant, DelegationAmountTooLarge
- enum system::CallerTag
- enum system::Caller
- enum system::handle_payment::Error
- enum system::handle_payment::Error has new variants IncompatiblePaymentSettings, UnexpectedKeyVariant
- enum system::mint::BalanceHoldAddrTag
- enum system::mint::Error has new variant: ForgedReference
- enum system::reservation::ReservationKind
- method CLValue::to_t
- function handle_stored_dictionary_value
- in arg_handling namespace functions: has_valid_activate_bid_args, has_valid_add_bid_args, has_valid_change_bid_public_key_args, has_valid_delegate_args, has_valid_redelegate_args, has_valid_transfer_args, has_valid_undelegate_args, has_valid_withdraw_bid_args, new_add_bid_args, new_delegate_args, new_redelegate_args, new_transfer_args, new_undelegate_args, new_withdraw_bid_args
- methods in ContractWasm: `new` and `take_bytes`
- method `lock_status` in struct ContractPackage
- function bytesrepr::allocate_buffer_for_size(expected_size: usize) -> Result<Vec<u8>, Error>
- Enum EntryPointAccess has new variant `Template` added

### Changed

- pub enum ApiError has new variants: MessageTopicAlreadyRegistered, MaxTopicsNumberExceeded, MaxTopicNameSizeExceeded, MessageTopicNotRegistered, MessageTopicFull, MessageTooLarge, MaxMessagesPerBlockExceeded,NotAllowedToAddContractVersion,InvalidDelegationAmountLimits,InvalidCallerInfoRequest
- struct AuctionState#bids is now a BTreeMap<PublicKey, Bid> instead of Vec<JsonBids>. This field is still serialized as an array. Due to this change the elements of the array will have more fields than before (added `validator_public_key`, `vesting_schedule`).
- Variants of enum EntryPointType changed
- Struct Parameter moved from contracts to addressable_entity::entry_points
- struct EraId has new methods `iter_range_inclusive`, `increment`
- struct ExecutionEffect moved to module execution::execution_result_v1
- enum OpKind moved to module execution::execution_result_v1
- struct Operation moved to module execution::execution_result_v1
- enum Transform changed name to TransformKindV1, moved to module execution::execution_result_v1 and has new variants (WriteAddressableEntity, Prune, WriteBidKind)
- enum ExecutionResult changed name to ExecutionResultV1, moved to module execution::execution_result_v1
- struct TransformEntry changed name to TransformV1 and moved to module execution::execution_result_v1
- moved NamedKey to module execution::execution_result_v1
- KeyTag::SystemContractRegistry variant changed name to KeyTag::SystemEntityRegistry
- variants for KeyTag enum: BidAddr = 15, Package = 16, AddressableEntity = 17, ByteCode = 18, Message = 19, NamedKey = 20, BlockGlobal = 21, BalanceHold = 22, EntryPoint = 23,
- enum Key::SystemContractRegistry changed name to Key::SystemEntityRegistry
- variants for enum Key: BidAddr, Package, AddressableEntity, ByteCode, Message, NamedKey, BlockGlobal, BalanceHold, EntryPoint,
- struct ExcessiveSizeError changed name to DeployExcessiveSizeError
- struct Transfer changed name to TransferV1
- enum GlobalStateIdentifier
- enum StoredValue has new variants: LegacyTransfer, AddressableEntity, BidKind, Package, ByteCode, MessageTopic, Message, NamedKey,Reservation,EntryPoint,
- enum system::SystemContractType changed name to system::SystemEntityType
- enum system::handle_payment::Error variant SystemFunctionCalledByUserAccount changed to InvalidCaller
- struct EntryPoint has a new field `entry_point_payment`
- struct BlockHeader was renamed to BlockHeaderV1 and used as a variant in enum BlockHeader
- struct Block was renamed to BlockV1 and used as a variant in enum Block
- Gas::from_motes now takes `u8` instead of `u64` as second parameter

### Removed

- type Groups (there is now a struct with that name)
- type EntryPointsMap
- type NamedKeys
- methods `groups_mut`, `add_group`, `lookup_contract_hash`, `is_version_enabled`, `is_contract_enabled`, `insert_contract_version`, `disable_contract_version`, `enable_contract_version`, `enabled_versions`, `remove_group`, `next_contract_version_for`, `current_contract_version`, `current_contract_hash` in struct ContractPackage
- in enum StoredValue removed variant Transfer (replaced with LegacyTransfer)

## [Unreleased] (node 1.5.4)

### Changed

- Remove filesystem I/O functionality from the `std` feature, and gated this behind a new feature `std-fs-io` which depends upon `std`.

## 4.0.1

### Added

- Add a new `SyncHandling` enum, which allows a node to opt out of historical sync.

### Changed

- Update `k256` to version 0.13.1.

### Removed

- Remove `ExecutionResult::successful_transfers`.

### Security

- Update `ed25519-dalek` to version 2.0.0 as mitigation for [RUSTSEC-2022-0093](https://rustsec.org/advisories/RUSTSEC-2022-0093)

## 3.0.0

### Added

- Add new `bytesrepr::Error::NotRepresentable` error variant that represents values that are not representable by the serialization format.
- Add new `Key::Unbond` key variant under which the new unbonding information (to support redelegation) is written.
- Add new `Key::ChainspecRegistry` key variant under which the `ChainspecRegistry` is written.
- Add new `Key::ChecksumRegistry` key variant under which a registry of checksums for a given block is written. There are two checksums in the registry, one for the execution results and the other for the approvals of all deploys in the block.
- Add new `StoredValue::Unbonding` variant to support redelegating.
- Add a new type `WithdrawPurses` which is meant to represent `UnbondingPurses` as they exist in current live networks.

### Changed

- Extend `UnbondingPurse` to take a new field `new_validator` which represents the validator to whom tokens will be re-delegated.
- Increase `DICTIONARY_ITEM_KEY_MAX_LENGTH` to 128.
- Change prefix of formatted string representation of `ContractPackageHash` from "contract-package-wasm" to "contract-package-". Parsing from the old format is still supported.
- Apply `#[non_exhaustive]` to error enums.
- Change Debug output of `DeployHash` to hex-encoded string rather than a list of integers.

### Fixed

- Fix some integer casts, where failure is now detected and reported via new error variant `NotRepresentable`.

## 2.0.0

### Fixed

- Republish v1.6.0 as v2.0.0 due to missed breaking change in API (addition of new variant to `Key`).

## 1.6.0 [YANKED]

### Added

- Extend asymmetric key functionality, available via feature `std` (moved from `casper-nodes` crate).
- Provide `Timestamp` and `TimeDiff` types for time operations, with extended functionality available via feature `std` (moved from `casper-nodes` crate).
- Provide test-only functionality, in particular a seedable RNG `TestRng` which outputs its seed on test failure. Available via a new feature `testing`.
- Add new `Key::EraSummary` key variant under which the era summary info is written on each switch block execution.

### Deprecated

- Deprecate `gens` feature: its functionality is included in the new `testing` feature.

## 1.5.0

### Added

- Provide types and functionality to support improved access control inside execution engine.
- Provide `CLTyped` impl for `ContractPackage` to allow it to be passed into contracts.

### Fixed

- Limit parsing of CLTyped objects to a maximum of 50 types deep.

## 1.4.6 - 2021-12-29

### Changed

- Disable checksummed-hex encoding, but leave checksummed-hex decoding in place.

## 1.4.5 - 2021-12-06

### Added

- Add function to `auction::MintProvider` trait to support minting into an existing purse.

### Changed

- Change checksummed hex implementation to use 32 byte rather than 64 byte blake2b digests.

## [1.4.4] - 2021-11-18

### Fixed

- Revert the accidental change to the `std` feature causing a broken build when this feature is enabled.

## [1.4.3] - 2021-11-17 [YANKED]

## [1.4.2] - 2021-11-13 [YANKED]

### Added

- Add checksummed hex encoding following a scheme similar to [EIP-55](https://eips.ethereum.org/EIPS/eip-55).

## [1.4.1] - 2021-10-23

No changes.

## [1.4.0] - 2021-10-21 [YANKED]

### Added

- Add `json-schema` feature, disabled by default, to enable many types to be used to produce JSON-schema data.
- Add implicit `datasize` feature, disabled by default, to enable many types to derive the `DataSize` trait.
- Add `StoredValue` types to this crate.

### Changed

- Support building and testing using stable Rust.
- Allow longer hex string to be presented in `json` files. Current maximum is increased from 100 to 150 characters.
- Improve documentation and `Debug` impls for `ApiError`.

### Deprecated

- Feature `std` is deprecated as it is now a no-op, since there is no benefit to linking the std lib via this crate.

## [1.3.0] - 2021-07-19

### Changed

- Restrict summarization when JSON pretty-printing to contiguous long hex strings.
- Update pinned version of Rust to `nightly-2021-06-17`.

### Removed

- Remove ability to clone `SecretKey`s.

## [1.2.0] - 2021-05-27

### Changed

- Change to Apache 2.0 license.
- Return a `Result` from the constructor of `SecretKey` rather than potentially panicking.
- Improve `Key` error reporting and tests.

### Fixed

- Fix `Key` deserialization.

## [1.1.1] - 2021-04-19

No changes.

## [1.1.0] - 2021-04-13 [YANKED]

No changes.

## [1.0.1] - 2021-04-08

No changes.

## [1.0.0] - 2021-03-30

### Added

- Initial release of types for use by software compatible with Casper mainnet.

[Keep a Changelog]: https://keepachangelog.com/en/1.0.0
[unreleased]: https://github.com/casper-network/casper-node/compare/24fc4027a...dev
[1.4.3]: https://github.com/casper-network/casper-node/compare/2be27b3f5...24fc4027a
[1.4.2]: https://github.com/casper-network/casper-node/compare/v1.4.1...2be27b3f5
[1.4.1]: https://github.com/casper-network/casper-node/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/casper-network/casper-node/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/casper-network/casper-node/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/casper-network/casper-node/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.1.0]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.0.1]: https://github.com/casper-network/casper-node/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/casper-network/casper-node/releases/tag/v1.0.0
