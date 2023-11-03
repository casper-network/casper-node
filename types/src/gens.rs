//! Contains functions for generating arbitrary values for use by
//! [`Proptest`](https://crates.io/crates/proptest).
#![allow(missing_docs)]

use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec,
};

use proptest::{
    array, bits, bool,
    collection::{self, SizeRange},
    option,
    prelude::*,
    result,
};

use crate::{
    account::{self, action_thresholds::gens::account_action_thresholds_arb, AccountHash},
    addressable_entity::{MessageTopics, NamedKeys, Parameters, Weight},
    contract_messages::{MessageChecksum, MessageTopicSummary, TopicNameHash},
    crypto::{self, gens::public_key_arb_no_system},
    package::{EntityVersionKey, EntityVersions, Groups, PackageStatus},
    system::auction::{
        gens::era_info_arb, DelegationRate, Delegator, UnbondingPurse, WithdrawPurse,
        DELEGATION_RATE_DENOMINATOR,
    },
    transfer::TransferAddr,
    AccessRights, AddressableEntity, AddressableEntityHash, BlockTime, ByteCode, CLType, CLValue,
    EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, EraId, Group, Key, NamedArg,
    Package, Parameter, Phase, ProtocolVersion, SemVer, StoredValue, URef, U128, U256, U512,
};

use crate::{
    account::{associated_keys::gens::account_associated_keys_arb, Account},
    addressable_entity::{
        action_thresholds::gens::action_thresholds_arb, associated_keys::gens::associated_keys_arb,
    },
    byte_code::ByteCodeKind,
    contracts::{
        Contract, ContractHash, ContractPackage, ContractPackageStatus, ContractVersionKey,
        ContractVersions,
    },
    deploy_info::gens::{deploy_hash_arb, transfer_addr_arb},
    package::PackageKind,
    system::auction::{Bid, BidAddr, BidKind, ValidatorBid},
};
pub use crate::{deploy_info::gens::deploy_info_arb, transfer::gens::transfer_arb};

pub fn u8_slice_32() -> impl Strategy<Value = [u8; 32]> {
    collection::vec(any::<u8>(), 32).prop_map(|b| {
        let mut res = [0u8; 32];
        res.clone_from_slice(b.as_slice());
        res
    })
}

pub fn u2_slice_32() -> impl Strategy<Value = [u8; 32]> {
    array::uniform32(any::<u8>()).prop_map(|mut arr| {
        for byte in arr.iter_mut() {
            *byte &= 0b11;
        }
        arr
    })
}

pub(crate) fn named_keys_arb(depth: usize) -> impl Strategy<Value = NamedKeys> {
    collection::btree_map("\\PC*", key_arb(), depth).prop_map(NamedKeys::from)
}

pub fn access_rights_arb() -> impl Strategy<Value = AccessRights> {
    prop_oneof![
        Just(AccessRights::NONE),
        Just(AccessRights::READ),
        Just(AccessRights::ADD),
        Just(AccessRights::WRITE),
        Just(AccessRights::READ_ADD),
        Just(AccessRights::READ_WRITE),
        Just(AccessRights::ADD_WRITE),
        Just(AccessRights::READ_ADD_WRITE),
    ]
}

pub fn phase_arb() -> impl Strategy<Value = Phase> {
    prop_oneof![
        Just(Phase::Payment),
        Just(Phase::Session),
        Just(Phase::FinalizePayment),
    ]
}

pub fn uref_arb() -> impl Strategy<Value = URef> {
    (array::uniform32(bits::u8::ANY), access_rights_arb())
        .prop_map(|(id, access_rights)| URef::new(id, access_rights))
}

pub fn era_id_arb() -> impl Strategy<Value = EraId> {
    any::<u64>().prop_map(EraId::from)
}

pub fn key_arb() -> impl Strategy<Value = Key> {
    prop_oneof![
        account_hash_arb().prop_map(Key::Account),
        u8_slice_32().prop_map(Key::Hash),
        uref_arb().prop_map(Key::URef),
        transfer_addr_arb().prop_map(Key::Transfer),
        deploy_hash_arb().prop_map(Key::DeployInfo),
        era_id_arb().prop_map(Key::EraInfo),
        uref_arb().prop_map(|uref| Key::Balance(uref.addr())),
        bid_addr_validator_arb().prop_map(Key::BidAddr),
        bid_addr_delegator_arb().prop_map(Key::BidAddr),
        account_hash_arb().prop_map(Key::Withdraw),
        u8_slice_32().prop_map(Key::Dictionary),
        Just(Key::EraSummary),
    ]
}

pub fn colliding_key_arb() -> impl Strategy<Value = Key> {
    prop_oneof![
        u2_slice_32().prop_map(|bytes| Key::Account(AccountHash::new(bytes))),
        u2_slice_32().prop_map(Key::Hash),
        u2_slice_32().prop_map(|bytes| Key::URef(URef::new(bytes, AccessRights::NONE))),
        u2_slice_32().prop_map(|bytes| Key::Transfer(TransferAddr::new(bytes))),
        u2_slice_32().prop_map(Key::Dictionary),
    ]
}

pub fn account_hash_arb() -> impl Strategy<Value = AccountHash> {
    u8_slice_32().prop_map(AccountHash::new)
}

pub fn bid_addr_validator_arb() -> impl Strategy<Value = BidAddr> {
    u8_slice_32().prop_map(BidAddr::new_validator_addr)
}

pub fn bid_addr_delegator_arb() -> impl Strategy<Value = BidAddr> {
    let x = u8_slice_32();
    let y = u8_slice_32();
    (x, y).prop_map(BidAddr::new_delegator_addr)
}

pub fn weight_arb() -> impl Strategy<Value = Weight> {
    any::<u8>().prop_map(Weight::new)
}

pub fn account_weight_arb() -> impl Strategy<Value = account::Weight> {
    any::<u8>().prop_map(account::Weight::new)
}

pub fn sem_ver_arb() -> impl Strategy<Value = SemVer> {
    (any::<u32>(), any::<u32>(), any::<u32>())
        .prop_map(|(major, minor, patch)| SemVer::new(major, minor, patch))
}

pub fn protocol_version_arb() -> impl Strategy<Value = ProtocolVersion> {
    sem_ver_arb().prop_map(ProtocolVersion::new)
}

pub fn u128_arb() -> impl Strategy<Value = U128> {
    collection::vec(any::<u8>(), 0..16).prop_map(|b| U128::from_little_endian(b.as_slice()))
}

pub fn u256_arb() -> impl Strategy<Value = U256> {
    collection::vec(any::<u8>(), 0..32).prop_map(|b| U256::from_little_endian(b.as_slice()))
}

pub fn u512_arb() -> impl Strategy<Value = U512> {
    prop_oneof![
        1 => Just(U512::zero()),
        8 => collection::vec(any::<u8>(), 0..64).prop_map(|b| U512::from_little_endian(b.as_slice())),
        1 => Just(U512::MAX),
    ]
}

pub fn cl_simple_type_arb() -> impl Strategy<Value = CLType> {
    prop_oneof![
        Just(CLType::Bool),
        Just(CLType::I32),
        Just(CLType::I64),
        Just(CLType::U8),
        Just(CLType::U32),
        Just(CLType::U64),
        Just(CLType::U128),
        Just(CLType::U256),
        Just(CLType::U512),
        Just(CLType::Unit),
        Just(CLType::String),
        Just(CLType::Key),
        Just(CLType::URef),
    ]
}

pub fn cl_type_arb() -> impl Strategy<Value = CLType> {
    cl_simple_type_arb().prop_recursive(4, 16, 8, |element| {
        prop_oneof![
            // We want to produce basic types too
            element.clone(),
            // For complex type
            element
                .clone()
                .prop_map(|val| CLType::Option(Box::new(val))),
            element.clone().prop_map(|val| CLType::List(Box::new(val))),
            // Realistic Result type generator: ok is anything recursive, err is simple type
            (element.clone(), cl_simple_type_arb()).prop_map(|(ok, err)| CLType::Result {
                ok: Box::new(ok),
                err: Box::new(err)
            }),
            // Realistic Map type generator: key is simple type, value is complex recursive type
            (cl_simple_type_arb(), element.clone()).prop_map(|(key, value)| CLType::Map {
                key: Box::new(key),
                value: Box::new(value)
            }),
            // Various tuples
            element
                .clone()
                .prop_map(|cl_type| CLType::Tuple1([Box::new(cl_type)])),
            (element.clone(), element.clone()).prop_map(|(cl_type1, cl_type2)| CLType::Tuple2([
                Box::new(cl_type1),
                Box::new(cl_type2)
            ])),
            (element.clone(), element.clone(), element).prop_map(
                |(cl_type1, cl_type2, cl_type3)| CLType::Tuple3([
                    Box::new(cl_type1),
                    Box::new(cl_type2),
                    Box::new(cl_type3)
                ])
            ),
        ]
    })
}

pub fn cl_value_arb() -> impl Strategy<Value = CLValue> {
    // If compiler brings you here it most probably means you've added a variant to `CLType` enum
    // but forgot to add generator for it.
    let stub: Option<CLType> = None;
    if let Some(cl_type) = stub {
        match cl_type {
            CLType::Bool
            | CLType::I32
            | CLType::I64
            | CLType::U8
            | CLType::U32
            | CLType::U64
            | CLType::U128
            | CLType::U256
            | CLType::U512
            | CLType::Unit
            | CLType::String
            | CLType::Key
            | CLType::URef
            | CLType::PublicKey
            | CLType::Option(_)
            | CLType::List(_)
            | CLType::ByteArray(..)
            | CLType::Result { .. }
            | CLType::Map { .. }
            | CLType::Tuple1(_)
            | CLType::Tuple2(_)
            | CLType::Tuple3(_)
            | CLType::Any => (),
        }
    };

    prop_oneof![
        Just(CLValue::from_t(()).expect("should create CLValue")),
        any::<bool>().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        any::<i32>().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        any::<i64>().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        any::<u8>().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        any::<u32>().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        any::<u64>().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        u128_arb().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        u256_arb().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        u512_arb().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        key_arb().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        uref_arb().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        ".*".prop_map(|x: String| CLValue::from_t(x).expect("should create CLValue")),
        option::of(any::<u64>()).prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        collection::vec(uref_arb(), 0..100)
            .prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        result::maybe_err(key_arb(), ".*")
            .prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        collection::btree_map(".*", u512_arb(), 0..100)
            .prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        (any::<bool>()).prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        (any::<bool>(), any::<i32>())
            .prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        (any::<bool>(), any::<i32>(), any::<i64>())
            .prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
        // Fixed lists of any size
        any::<u8>().prop_map(|len| CLValue::from_t([len; 32]).expect("should create CLValue")),
    ]
}

pub fn result_arb() -> impl Strategy<Value = Result<u32, u32>> {
    result::maybe_ok(any::<u32>(), any::<u32>())
}

pub fn named_args_arb() -> impl Strategy<Value = NamedArg> {
    (".*", cl_value_arb()).prop_map(|(name, value)| NamedArg::new(name, value))
}

pub fn group_arb() -> impl Strategy<Value = Group> {
    ".*".prop_map(Group::new)
}

pub fn entry_point_access_arb() -> impl Strategy<Value = EntryPointAccess> {
    prop_oneof![
        Just(EntryPointAccess::Public),
        collection::vec(group_arb(), 0..32).prop_map(EntryPointAccess::Groups),
        Just(EntryPointAccess::Template),
    ]
}

pub fn entry_point_type_arb() -> impl Strategy<Value = EntryPointType> {
    prop_oneof![
        Just(EntryPointType::Session),
        Just(EntryPointType::AddressableEntity),
        Just(EntryPointType::Factory),
    ]
}

pub fn parameter_arb() -> impl Strategy<Value = Parameter> {
    (".*", cl_type_arb()).prop_map(|(name, cl_type)| Parameter::new(name, cl_type))
}

pub fn parameters_arb() -> impl Strategy<Value = Parameters> {
    collection::vec(parameter_arb(), 0..10)
}

pub fn entry_point_arb() -> impl Strategy<Value = EntryPoint> {
    (
        ".*",
        parameters_arb(),
        entry_point_type_arb(),
        entry_point_access_arb(),
        cl_type_arb(),
    )
        .prop_map(
            |(name, parameters, entry_point_type, entry_point_access, ret)| {
                EntryPoint::new(name, parameters, ret, entry_point_access, entry_point_type)
            },
        )
}

pub fn entry_points_arb() -> impl Strategy<Value = EntryPoints> {
    collection::vec(entry_point_arb(), 1..10).prop_map(EntryPoints::from)
}

pub fn message_topics_arb() -> impl Strategy<Value = MessageTopics> {
    collection::vec(any::<String>(), 1..100).prop_map(|topic_names| {
        MessageTopics::from(
            topic_names
                .into_iter()
                .map(|name| {
                    let name_hash = crypto::blake2b(&name).into();
                    (name, name_hash)
                })
                .collect::<BTreeMap<String, TopicNameHash>>(),
        )
    })
}

pub fn account_arb() -> impl Strategy<Value = Account> {
    (
        account_hash_arb(),
        named_keys_arb(20),
        uref_arb(),
        account_associated_keys_arb(),
        account_action_thresholds_arb(),
    )
        .prop_map(
            |(account_hash, named_keys, main_purse, associated_keys, action_thresholds)| {
                Account::new(
                    account_hash,
                    named_keys,
                    main_purse,
                    associated_keys,
                    action_thresholds,
                )
            },
        )
}

pub fn contract_package_arb() -> impl Strategy<Value = ContractPackage> {
    (
        uref_arb(),
        contract_versions_arb(),
        disabled_contract_versions_arb(),
        groups_arb(),
    )
        .prop_map(|(access_key, versions, disabled_versions, groups)| {
            ContractPackage::new(
                access_key,
                versions,
                disabled_versions,
                groups,
                ContractPackageStatus::default(),
            )
        })
}

pub fn contract_arb() -> impl Strategy<Value = Contract> {
    (
        protocol_version_arb(),
        entry_points_arb(),
        u8_slice_32(),
        u8_slice_32(),
        named_keys_arb(20),
    )
        .prop_map(
            |(
                protocol_version,
                entry_points,
                contract_package_hash_arb,
                contract_wasm_hash,
                named_keys,
            )| {
                Contract::new(
                    contract_package_hash_arb.into(),
                    contract_wasm_hash.into(),
                    named_keys,
                    entry_points,
                    protocol_version,
                )
            },
        )
}

pub fn addressable_entity_arb() -> impl Strategy<Value = AddressableEntity> {
    (
        protocol_version_arb(),
        entry_points_arb(),
        u8_slice_32(),
        u8_slice_32(),
        named_keys_arb(20),
        uref_arb(),
        associated_keys_arb(),
        action_thresholds_arb(),
        message_topics_arb(),
    )
        .prop_map(
            |(
                protocol_version,
                entry_points,
                contract_package_hash_arb,
                contract_wasm_hash,
                named_keys,
                main_purse,
                associated_keys,
                action_thresholds,
                message_topics,
            )| {
                AddressableEntity::new(
                    contract_package_hash_arb.into(),
                    contract_wasm_hash.into(),
                    named_keys,
                    entry_points,
                    protocol_version,
                    main_purse,
                    associated_keys,
                    action_thresholds,
                    message_topics,
                )
            },
        )
}

pub fn byte_code_arb() -> impl Strategy<Value = ByteCode> {
    collection::vec(any::<u8>(), 1..1000)
        .prop_map(|byte_code| ByteCode::new(ByteCodeKind::V1CasperWasm, byte_code))
}

pub fn contract_version_key_arb() -> impl Strategy<Value = ContractVersionKey> {
    (1..32u32, 1..1000u32)
        .prop_map(|(major, contract_ver)| ContractVersionKey::new(major, contract_ver))
}

pub fn entity_version_key_arb() -> impl Strategy<Value = EntityVersionKey> {
    (1..32u32, 1..1000u32)
        .prop_map(|(major, contract_ver)| EntityVersionKey::new(major, contract_ver))
}

pub fn contract_versions_arb() -> impl Strategy<Value = ContractVersions> {
    collection::btree_map(
        contract_version_key_arb(),
        u8_slice_32().prop_map(ContractHash::new),
        1..5,
    )
}

pub fn entity_versions_arb() -> impl Strategy<Value = EntityVersions> {
    collection::btree_map(
        entity_version_key_arb(),
        u8_slice_32().prop_map(AddressableEntityHash::new),
        1..5,
    )
    .prop_map(EntityVersions::from)
}

pub fn disabled_versions_arb() -> impl Strategy<Value = BTreeSet<EntityVersionKey>> {
    collection::btree_set(entity_version_key_arb(), 0..5)
}

pub fn disabled_contract_versions_arb() -> impl Strategy<Value = BTreeSet<ContractVersionKey>> {
    collection::btree_set(contract_version_key_arb(), 0..5)
}

pub fn groups_arb() -> impl Strategy<Value = Groups> {
    collection::btree_map(group_arb(), collection::btree_set(uref_arb(), 1..10), 0..5)
        .prop_map(Groups::from)
}

pub fn package_arb() -> impl Strategy<Value = Package> {
    (
        uref_arb(),
        entity_versions_arb(),
        disabled_versions_arb(),
        groups_arb(),
    )
        .prop_map(|(access_key, versions, disabled_versions, groups)| {
            Package::new(
                access_key,
                versions,
                disabled_versions,
                groups,
                PackageStatus::default(),
                PackageKind::SmartContract,
            )
        })
}

pub(crate) fn delegator_arb() -> impl Strategy<Value = Delegator> {
    (
        public_key_arb_no_system(),
        u512_arb(),
        uref_arb(),
        public_key_arb_no_system(),
    )
        .prop_map(
            |(delegator_pk, staked_amount, bonding_purse, validator_pk)| {
                Delegator::unlocked(delegator_pk, staked_amount, bonding_purse, validator_pk)
            },
        )
}

fn delegation_rate_arb() -> impl Strategy<Value = DelegationRate> {
    0..=DELEGATION_RATE_DENOMINATOR // Maximum, allowed value for delegation rate.
}

pub(crate) fn unified_bid_arb(
    delegations_len: impl Into<SizeRange>,
) -> impl Strategy<Value = BidKind> {
    (
        public_key_arb_no_system(),
        uref_arb(),
        u512_arb(),
        delegation_rate_arb(),
        bool::ANY,
        collection::vec(delegator_arb(), delegations_len),
    )
        .prop_map(
            |(
                validator_public_key,
                bonding_purse,
                staked_amount,
                delegation_rate,
                is_locked,
                new_delegators,
            )| {
                let mut bid = if is_locked {
                    Bid::locked(
                        validator_public_key,
                        bonding_purse,
                        staked_amount,
                        delegation_rate,
                        1u64,
                    )
                } else {
                    Bid::unlocked(
                        validator_public_key,
                        bonding_purse,
                        staked_amount,
                        delegation_rate,
                    )
                };
                let delegators = bid.delegators_mut();
                new_delegators.into_iter().for_each(|delegator| {
                    assert!(delegators
                        .insert(delegator.delegator_public_key().clone(), delegator)
                        .is_none());
                });
                BidKind::Unified(Box::new(bid))
            },
        )
}

pub(crate) fn delegator_bid_arb() -> impl Strategy<Value = BidKind> {
    (delegator_arb()).prop_map(|delegator| BidKind::Delegator(Box::new(delegator)))
}

pub(crate) fn validator_bid_arb() -> impl Strategy<Value = BidKind> {
    (
        public_key_arb_no_system(),
        uref_arb(),
        u512_arb(),
        delegation_rate_arb(),
        bool::ANY,
    )
        .prop_map(
            |(validator_public_key, bonding_purse, staked_amount, delegation_rate, is_locked)| {
                let validator_bid = if is_locked {
                    ValidatorBid::locked(
                        validator_public_key,
                        bonding_purse,
                        staked_amount,
                        delegation_rate,
                        1u64,
                    )
                } else {
                    ValidatorBid::unlocked(
                        validator_public_key,
                        bonding_purse,
                        staked_amount,
                        delegation_rate,
                    )
                };
                BidKind::Validator(Box::new(validator_bid))
            },
        )
}

fn withdraw_arb() -> impl Strategy<Value = WithdrawPurse> {
    (
        uref_arb(),
        public_key_arb_no_system(),
        public_key_arb_no_system(),
        era_id_arb(),
        u512_arb(),
    )
        .prop_map(|(bonding_purse, validator_pk, unbonder_pk, era, amount)| {
            WithdrawPurse::new(bonding_purse, validator_pk, unbonder_pk, era, amount)
        })
}

fn withdraws_arb(size: impl Into<SizeRange>) -> impl Strategy<Value = Vec<WithdrawPurse>> {
    collection::vec(withdraw_arb(), size)
}

fn unbonding_arb() -> impl Strategy<Value = UnbondingPurse> {
    (
        uref_arb(),
        public_key_arb_no_system(),
        public_key_arb_no_system(),
        era_id_arb(),
        u512_arb(),
        option::of(public_key_arb_no_system()),
    )
        .prop_map(
            |(
                bonding_purse,
                validator_public_key,
                unbonder_public_key,
                era,
                amount,
                new_validator,
            )| {
                UnbondingPurse::new(
                    bonding_purse,
                    validator_public_key,
                    unbonder_public_key,
                    era,
                    amount,
                    new_validator,
                )
            },
        )
}

fn unbondings_arb(size: impl Into<SizeRange>) -> impl Strategy<Value = Vec<UnbondingPurse>> {
    collection::vec(unbonding_arb(), size)
}

fn message_topic_summary_arb() -> impl Strategy<Value = MessageTopicSummary> {
    (any::<u32>(), any::<u64>()).prop_map(|(message_count, blocktime)| MessageTopicSummary {
        message_count,
        blocktime: BlockTime::new(blocktime),
    })
}

fn message_summary_arb() -> impl Strategy<Value = MessageChecksum> {
    u8_slice_32().prop_map(MessageChecksum)
}

pub fn stored_value_arb() -> impl Strategy<Value = StoredValue> {
    prop_oneof![
        cl_value_arb().prop_map(StoredValue::CLValue),
        account_arb().prop_map(StoredValue::Account),
        byte_code_arb().prop_map(StoredValue::ByteCode),
        contract_arb().prop_map(StoredValue::Contract),
        addressable_entity_arb().prop_map(StoredValue::AddressableEntity),
        package_arb().prop_map(StoredValue::Package),
        transfer_arb().prop_map(StoredValue::Transfer),
        deploy_info_arb().prop_map(StoredValue::DeployInfo),
        era_info_arb(1..10).prop_map(StoredValue::EraInfo),
        unified_bid_arb(0..3).prop_map(StoredValue::BidKind),
        validator_bid_arb().prop_map(StoredValue::BidKind),
        delegator_bid_arb().prop_map(StoredValue::BidKind),
        withdraws_arb(1..50).prop_map(StoredValue::Withdraw),
        unbondings_arb(1..50).prop_map(StoredValue::Unbonding),
        message_topic_summary_arb().prop_map(StoredValue::MessageTopic),
        message_summary_arb().prop_map(StoredValue::Message),
    ]
    .prop_map(|stored_value|
        // The following match statement is here only to make sure
        // we don't forget to update the generator when a new variant is added.
        match stored_value {
            StoredValue::CLValue(_) => stored_value,
            StoredValue::Account(_) => stored_value,
            StoredValue::ContractWasm(_) => stored_value,
            StoredValue::Contract(_) => stored_value,
            StoredValue::ContractPackage(_) => stored_value,
            StoredValue::Transfer(_) => stored_value,
            StoredValue::DeployInfo(_) => stored_value,
            StoredValue::EraInfo(_) => stored_value,
            StoredValue::Bid(_) => stored_value,
            StoredValue::Withdraw(_) => stored_value,
            StoredValue::Unbonding(_) => stored_value,
            StoredValue::AddressableEntity(_) => stored_value,
            StoredValue::BidKind(_) => stored_value,
            StoredValue::Package(_) => stored_value,
            StoredValue::ByteCode(_) => stored_value,
            StoredValue::MessageTopic(_) => stored_value,
            StoredValue::Message(_) => stored_value,
        })
}
