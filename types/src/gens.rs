//! Contains functions for generating arbitrary values for use by
//! [`Proptest`](https://crates.io/crates/proptest).
#![allow(missing_docs)]
use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec,
};

use crate::{
    account::{
        self, action_thresholds::gens::account_action_thresholds_arb,
        associated_keys::gens::account_associated_keys_arb, Account, AccountHash,
    },
    addressable_entity::{
        action_thresholds::gens::action_thresholds_arb, associated_keys::gens::associated_keys_arb,
        ContractRuntimeTag, MessageTopics, NamedKeyAddr, NamedKeyValue, Parameters, Weight,
    },
    block::BlockGlobalAddr,
    byte_code::ByteCodeKind,
    bytesrepr::Bytes,
    contract_messages::{MessageAddr, MessageChecksum, MessageTopicSummary, TopicNameHash},
    contracts::{
        Contract, ContractHash, ContractPackage, ContractPackageStatus, ContractVersionKey,
        ContractVersions, EntryPoint as ContractEntryPoint, EntryPoints as ContractEntryPoints,
        NamedKeys,
    },
    crypto::{
        self,
        gens::{public_key_arb_no_system, secret_key_arb_no_system},
    },
    deploy_info::gens::deploy_info_arb,
    global_state::{Pointer, TrieMerkleProof, TrieMerkleProofStep},
    package::{EntityVersionKey, EntityVersions, Groups, PackageStatus},
    system::{
        auction::{
            gens::era_info_arb, Bid, BidAddr, BidKind, DelegationRate, Delegator, DelegatorBid,
            DelegatorKind, Reservation, UnbondingPurse, ValidatorBid, ValidatorCredit,
            WithdrawPurse, DELEGATION_RATE_DENOMINATOR,
        },
        mint::BalanceHoldAddr,
        SystemEntityType,
    },
    transaction::{
        gens::deploy_hash_arb, FieldsContainer, InitiatorAddrAndSecretKey, TransactionArgs,
        TransactionRuntimeParams, TransactionV1Payload,
    },
    transfer::{
        gens::{transfer_v1_addr_arb, transfer_v1_arb},
        TransferAddr,
    },
    AccessRights, AddressableEntity, AddressableEntityHash, BlockTime, ByteCode, ByteCodeAddr,
    CLType, CLValue, Digest, EntityAddr, EntityKind, EntryPoint, EntryPointAccess, EntryPointAddr,
    EntryPointPayment, EntryPointType, EntryPoints, EraId, Group, InitiatorAddr, Key, NamedArg,
    Package, Parameter, Phase, PricingMode, ProtocolVersion, PublicKey, RuntimeArgs, SemVer,
    StoredValue, TimeDiff, Timestamp, Transaction, TransactionEntryPoint,
    TransactionInvocationTarget, TransactionScheduling, TransactionTarget, TransactionV1, URef,
    U128, U256, U512,
};
use proptest::{
    array, bits, bool,
    collection::{self, vec, SizeRange},
    option,
    prelude::*,
    result,
};

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

pub fn named_key_addr_arb() -> impl Strategy<Value = NamedKeyAddr> {
    (entity_addr_arb(), u8_slice_32())
        .prop_map(|(entity_addr, b)| NamedKeyAddr::new_named_key_entry(entity_addr, b))
}

pub fn message_addr_arb() -> impl Strategy<Value = MessageAddr> {
    prop_oneof![
        (u8_slice_32(), u8_slice_32()).prop_map(|(hash_addr, topic_name_hash)| {
            MessageAddr::new_topic_addr(hash_addr, TopicNameHash::new(topic_name_hash))
        }),
        (u8_slice_32(), u8_slice_32(), example_u32_arb()).prop_map(
            |(hash_addr, topic_name_hash, index)| MessageAddr::new_message_addr(
                hash_addr,
                TopicNameHash::new(topic_name_hash),
                index
            )
        ),
    ]
}

pub fn entry_point_addr_arb() -> impl Strategy<Value = EntryPointAddr> {
    (entity_addr_arb(), any::<String>()).prop_map(|(entity_addr, b)| {
        EntryPointAddr::new_v1_entry_point_addr(entity_addr, &b).unwrap()
    })
}

pub fn byte_code_addr_arb() -> impl Strategy<Value = ByteCodeAddr> {
    prop_oneof![
        Just(ByteCodeAddr::Empty),
        u8_slice_32().prop_map(ByteCodeAddr::V1CasperWasm),
        u8_slice_32().prop_map(ByteCodeAddr::V2CasperWasm),
    ]
}

pub fn key_arb() -> impl Strategy<Value = Key> {
    prop_oneof![
        account_hash_arb().prop_map(Key::Account),
        u8_slice_32().prop_map(Key::Hash),
        uref_arb().prop_map(Key::URef),
        transfer_v1_addr_arb().prop_map(Key::Transfer),
        deploy_hash_arb().prop_map(Key::DeployInfo),
        era_id_arb().prop_map(Key::EraInfo),
        uref_arb().prop_map(|uref| Key::Balance(uref.addr())),
        bid_addr_validator_arb().prop_map(Key::BidAddr),
        bid_addr_delegator_arb().prop_map(Key::BidAddr),
        account_hash_arb().prop_map(Key::Withdraw),
        u8_slice_32().prop_map(Key::Dictionary),
        balance_hold_addr_arb().prop_map(Key::BalanceHold),
        Just(Key::EraSummary)
    ]
}

pub fn all_keys_arb() -> impl Strategy<Value = Key> {
    prop_oneof![
        account_hash_arb().prop_map(Key::Account),
        u8_slice_32().prop_map(Key::Hash),
        uref_arb().prop_map(Key::URef),
        transfer_v1_addr_arb().prop_map(Key::Transfer),
        deploy_hash_arb().prop_map(Key::DeployInfo),
        era_id_arb().prop_map(Key::EraInfo),
        uref_arb().prop_map(|uref| Key::Balance(uref.addr())),
        account_hash_arb().prop_map(Key::Withdraw),
        u8_slice_32().prop_map(Key::Dictionary),
        balance_hold_addr_arb().prop_map(Key::BalanceHold),
        Just(Key::EraSummary),
        Just(Key::SystemEntityRegistry),
        Just(Key::ChainspecRegistry),
        Just(Key::ChecksumRegistry),
        bid_addr_arb().prop_map(Key::BidAddr),
        account_hash_arb().prop_map(Key::Bid),
        account_hash_arb().prop_map(Key::Unbond),
        u8_slice_32().prop_map(Key::SmartContract),
        byte_code_addr_arb().prop_map(Key::ByteCode),
        entity_addr_arb().prop_map(Key::AddressableEntity),
        block_global_addr_arb().prop_map(Key::BlockGlobal),
        message_addr_arb().prop_map(Key::Message),
        named_key_addr_arb().prop_map(Key::NamedKey),
        balance_hold_addr_arb().prop_map(Key::BalanceHold),
        entry_point_addr_arb().prop_map(Key::EntryPoint),
        entity_addr_arb().prop_map(Key::State),
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

pub fn entity_addr_arb() -> impl Strategy<Value = EntityAddr> {
    prop_oneof![
        u8_slice_32().prop_map(EntityAddr::System),
        u8_slice_32().prop_map(EntityAddr::Account),
        u8_slice_32().prop_map(EntityAddr::SmartContract),
    ]
}

pub fn topic_name_hash_arb() -> impl Strategy<Value = TopicNameHash> {
    u8_slice_32().prop_map(TopicNameHash::new)
}

pub fn bid_addr_validator_arb() -> impl Strategy<Value = BidAddr> {
    u8_slice_32().prop_map(BidAddr::new_validator_addr)
}

pub fn bid_addr_delegator_arb() -> impl Strategy<Value = BidAddr> {
    let x = u8_slice_32();
    let y = u8_slice_32();
    (x, y).prop_map(BidAddr::new_delegator_account_addr)
}

pub fn bid_legacy_arb() -> impl Strategy<Value = BidAddr> {
    u8_slice_32().prop_map(BidAddr::legacy)
}

pub fn bid_addr_delegated_arb() -> impl Strategy<Value = BidAddr> {
    (public_key_arb_no_system(), delegator_kind_arb()).prop_map(|(validator, delegator_kind)| {
        BidAddr::new_delegator_kind(&validator, &delegator_kind)
    })
}

pub fn bid_addr_credit_arb() -> impl Strategy<Value = BidAddr> {
    (public_key_arb_no_system(), era_id_arb())
        .prop_map(|(validator, era_id)| BidAddr::new_credit(&validator, era_id))
}

pub fn bid_addr_reservation_account_arb() -> impl Strategy<Value = BidAddr> {
    (public_key_arb_no_system(), public_key_arb_no_system())
        .prop_map(|(validator, delegator)| BidAddr::new_reservation_account(&validator, &delegator))
}

pub fn bid_addr_reservation_purse_arb() -> impl Strategy<Value = BidAddr> {
    (public_key_arb_no_system(), u8_slice_32())
        .prop_map(|(validator, uref)| BidAddr::new_reservation_purse(&validator, uref))
}

pub fn bid_addr_new_unbond_account_arb() -> impl Strategy<Value = BidAddr> {
    (public_key_arb_no_system(), public_key_arb_no_system())
        .prop_map(|(validator, unbonder)| BidAddr::new_unbond_account(validator, unbonder))
}

pub fn bid_addr_arb() -> impl Strategy<Value = BidAddr> {
    prop_oneof![
        bid_addr_validator_arb(),
        bid_addr_delegator_arb(),
        bid_legacy_arb(),
        bid_addr_delegated_arb(),
        bid_addr_credit_arb(),
        bid_addr_reservation_account_arb(),
        bid_addr_reservation_purse_arb(),
        bid_addr_new_unbond_account_arb(),
    ]
}

pub fn balance_hold_addr_arb() -> impl Strategy<Value = BalanceHoldAddr> {
    let x = uref_arb().prop_map(|uref| uref.addr());
    let y = any::<u64>();
    (x, y).prop_map(|(x, y)| BalanceHoldAddr::new_gas(x, BlockTime::new(y)))
}

pub fn block_global_addr_arb() -> impl Strategy<Value = BlockGlobalAddr> {
    prop_oneof![
        0 => Just(BlockGlobalAddr::BlockTime),
        1 => Just(BlockGlobalAddr::MessageCount)
    ]
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
        any::<bool>().prop_map(|x| CLValue::from_t(x).expect("should create CLValue")),
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
        Just(EntryPointType::Caller),
        Just(EntryPointType::Called),
        Just(EntryPointType::Factory),
    ]
}

pub fn entry_point_payment_arb() -> impl Strategy<Value = EntryPointPayment> {
    prop_oneof![
        Just(EntryPointPayment::Caller),
        Just(EntryPointPayment::DirectInvocationOnly),
        Just(EntryPointPayment::SelfOnward),
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
        entry_point_payment_arb(),
        cl_type_arb(),
    )
        .prop_map(
            |(name, parameters, entry_point_type, entry_point_access, entry_point_payment, ret)| {
                EntryPoint::new(
                    name,
                    parameters,
                    ret,
                    entry_point_access,
                    entry_point_type,
                    entry_point_payment,
                )
            },
        )
}

pub fn contract_entry_point_arb() -> impl Strategy<Value = ContractEntryPoint> {
    (
        ".*",
        parameters_arb(),
        entry_point_type_arb(),
        entry_point_access_arb(),
        cl_type_arb(),
    )
        .prop_map(
            |(name, parameters, entry_point_type, entry_point_access, ret)| {
                ContractEntryPoint::new(name, parameters, ret, entry_point_access, entry_point_type)
            },
        )
}

pub fn entry_points_arb() -> impl Strategy<Value = EntryPoints> {
    collection::vec(entry_point_arb(), 1..10).prop_map(EntryPoints::from)
}

pub fn contract_entry_points_arb() -> impl Strategy<Value = ContractEntryPoints> {
    collection::vec(contract_entry_point_arb(), 1..10).prop_map(ContractEntryPoints::from)
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
        contract_entry_points_arb(),
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

pub fn system_entity_type_arb() -> impl Strategy<Value = SystemEntityType> {
    prop_oneof![
        Just(SystemEntityType::Mint),
        Just(SystemEntityType::HandlePayment),
        Just(SystemEntityType::StandardPayment),
        Just(SystemEntityType::Auction),
    ]
}

pub fn contract_runtime_arb() -> impl Strategy<Value = ContractRuntimeTag> {
    prop_oneof![
        Just(ContractRuntimeTag::VmCasperV1),
        Just(ContractRuntimeTag::VmCasperV2),
    ]
}

pub fn entity_kind_arb() -> impl Strategy<Value = EntityKind> {
    prop_oneof![
        system_entity_type_arb().prop_map(EntityKind::System),
        account_hash_arb().prop_map(EntityKind::Account),
        contract_runtime_arb().prop_map(EntityKind::SmartContract),
    ]
}

pub fn addressable_entity_hash_arb() -> impl Strategy<Value = AddressableEntityHash> {
    u8_slice_32().prop_map(AddressableEntityHash::new)
}

pub fn addressable_entity_arb() -> impl Strategy<Value = AddressableEntity> {
    (
        protocol_version_arb(),
        u8_slice_32(),
        u8_slice_32(),
        uref_arb(),
        associated_keys_arb(),
        action_thresholds_arb(),
        entity_kind_arb(),
    )
        .prop_map(
            |(
                protocol_version,
                contract_package_hash_arb,
                contract_wasm_hash,
                main_purse,
                associated_keys,
                action_thresholds,
                entity_kind,
            )| {
                AddressableEntity::new(
                    contract_package_hash_arb.into(),
                    contract_wasm_hash.into(),
                    protocol_version,
                    main_purse,
                    associated_keys,
                    action_thresholds,
                    entity_kind,
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
    (entity_versions_arb(), disabled_versions_arb(), groups_arb()).prop_map(
        |(versions, disabled_versions, groups)| {
            Package::new(
                versions,
                disabled_versions,
                groups,
                PackageStatus::default(),
            )
        },
    )
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

pub(crate) fn delegator_kind_arb() -> impl Strategy<Value = DelegatorKind> {
    prop_oneof![
        public_key_arb_no_system().prop_map(DelegatorKind::PublicKey),
        array::uniform32(bits::u8::ANY).prop_map(DelegatorKind::Purse)
    ]
}

pub(crate) fn delegator_bid_arb() -> impl Strategy<Value = DelegatorBid> {
    (
        public_key_arb_no_system(),
        u512_arb(),
        uref_arb(),
        public_key_arb_no_system(),
    )
        .prop_map(
            |(delegator_pk, staked_amount, bonding_purse, validator_pk)| {
                DelegatorBid::unlocked(
                    delegator_pk.into(),
                    staked_amount,
                    bonding_purse,
                    validator_pk,
                )
            },
        )
}

fn delegation_rate_arb() -> impl Strategy<Value = DelegationRate> {
    0..=DELEGATION_RATE_DENOMINATOR // Maximum, allowed value for delegation rate.
}

pub(crate) fn reservation_bid_arb() -> impl Strategy<Value = BidKind> {
    reservation_arb().prop_map(|reservation| BidKind::Reservation(Box::new(reservation)))
}

pub(crate) fn reservation_arb() -> impl Strategy<Value = Reservation> {
    (
        public_key_arb_no_system(),
        delegator_kind_arb(),
        delegation_rate_arb(),
    )
        .prop_map(|(validator_pk, delegator_kind, delegation_rate)| {
            Reservation::new(validator_pk, delegator_kind, delegation_rate)
        })
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

pub(crate) fn delegator_bid_kind_arb() -> impl Strategy<Value = BidKind> {
    delegator_bid_arb().prop_map(|delegator| BidKind::Delegator(Box::new(delegator)))
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
                        0,
                        u64::MAX,
                        0,
                    )
                } else {
                    ValidatorBid::unlocked(
                        validator_public_key,
                        bonding_purse,
                        staked_amount,
                        delegation_rate,
                        0,
                        u64::MAX,
                        0,
                    )
                };
                BidKind::Validator(Box::new(validator_bid))
            },
        )
}

pub(crate) fn credit_bid_arb() -> impl Strategy<Value = BidKind> {
    (public_key_arb_no_system(), era_id_arb(), u512_arb()).prop_map(
        |(validator_public_key, era_id, amount)| {
            BidKind::Credit(Box::new(ValidatorCredit::new(
                validator_public_key,
                era_id,
                amount,
            )))
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
    (any::<u32>(), any::<u64>(), "test").prop_map(|(message_count, blocktime, topic_name)| {
        MessageTopicSummary {
            message_count,
            blocktime: BlockTime::new(blocktime),
            topic_name,
        }
    })
}

fn message_summary_arb() -> impl Strategy<Value = MessageChecksum> {
    u8_slice_32().prop_map(MessageChecksum)
}

pub fn named_key_value_arb() -> impl Strategy<Value = NamedKeyValue> {
    (key_arb(), "test").prop_map(|(key, string)| {
        let cl_key = CLValue::from_t(key).unwrap();
        let cl_string = CLValue::from_t(string).unwrap();
        NamedKeyValue::new(cl_key, cl_string)
    })
}

pub fn stored_value_arb() -> impl Strategy<Value = StoredValue> {
    prop_oneof![
        cl_value_arb().prop_map(StoredValue::CLValue),
        account_arb().prop_map(StoredValue::Account),
        byte_code_arb().prop_map(StoredValue::ByteCode),
        contract_arb().prop_map(StoredValue::Contract),
        contract_package_arb().prop_map(StoredValue::ContractPackage),
        addressable_entity_arb().prop_map(StoredValue::AddressableEntity),
        package_arb().prop_map(StoredValue::SmartContract),
        transfer_v1_arb().prop_map(StoredValue::Transfer),
        deploy_info_arb().prop_map(StoredValue::DeployInfo),
        era_info_arb(1..10).prop_map(StoredValue::EraInfo),
        unified_bid_arb(0..3).prop_map(StoredValue::BidKind),
        validator_bid_arb().prop_map(StoredValue::BidKind),
        delegator_bid_kind_arb().prop_map(StoredValue::BidKind),
        reservation_bid_arb().prop_map(StoredValue::BidKind),
        credit_bid_arb().prop_map(StoredValue::BidKind),
        withdraws_arb(1..50).prop_map(StoredValue::Withdraw),
        unbondings_arb(1..50).prop_map(StoredValue::Unbonding),
        message_topic_summary_arb().prop_map(StoredValue::MessageTopic),
        message_summary_arb().prop_map(StoredValue::Message),
        named_key_value_arb().prop_map(StoredValue::NamedKey),
        collection::vec(any::<u8>(), 0..1000).prop_map(StoredValue::RawBytes),
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
                StoredValue::SmartContract(_) => stored_value,
                StoredValue::ByteCode(_) => stored_value,
                StoredValue::MessageTopic(_) => stored_value,
                StoredValue::Message(_) => stored_value,
                StoredValue::NamedKey(_) => stored_value,
                StoredValue::Prepayment(_) => stored_value,
                StoredValue::EntryPoint(_) => stored_value,
                StoredValue::RawBytes(_) => stored_value,
        })
}

pub fn blake2b_hash_arb() -> impl Strategy<Value = Digest> {
    vec(any::<u8>(), 0..1000).prop_map(Digest::hash)
}

pub fn trie_pointer_arb() -> impl Strategy<Value = Pointer> {
    prop_oneof![
        blake2b_hash_arb().prop_map(Pointer::LeafPointer),
        blake2b_hash_arb().prop_map(Pointer::NodePointer)
    ]
}

pub fn trie_merkle_proof_step_arb() -> impl Strategy<Value = TrieMerkleProofStep> {
    const POINTERS_SIZE: usize = 32;
    const AFFIX_SIZE: usize = 6;

    prop_oneof![
        (
            <u8>::arbitrary(),
            vec((<u8>::arbitrary(), trie_pointer_arb()), POINTERS_SIZE)
        )
            .prop_map(|(hole_index, indexed_pointers_with_hole)| {
                TrieMerkleProofStep::Node {
                    hole_index,
                    indexed_pointers_with_hole,
                }
            }),
        vec(<u8>::arbitrary(), AFFIX_SIZE).prop_map(|affix| {
            TrieMerkleProofStep::Extension {
                affix: affix.into(),
            }
        })
    ]
}

pub fn trie_merkle_proof_arb() -> impl Strategy<Value = TrieMerkleProof<Key, StoredValue>> {
    const STEPS_SIZE: usize = 6;

    (
        key_arb(),
        stored_value_arb(),
        vec(trie_merkle_proof_step_arb(), STEPS_SIZE),
    )
        .prop_map(|(key, value, proof_steps)| TrieMerkleProof::new(key, value, proof_steps.into()))
}

pub fn transaction_scheduling_arb() -> impl Strategy<Value = TransactionScheduling> {
    prop_oneof![
        Just(TransactionScheduling::Standard),
        era_id_arb().prop_map(TransactionScheduling::FutureEra),
        any::<u64>().prop_map(
            |timestamp| TransactionScheduling::FutureTimestamp(Timestamp::from(timestamp))
        ),
    ]
}

pub fn json_compliant_transaction_scheduling_arb() -> impl Strategy<Value = TransactionScheduling> {
    prop_oneof![
        Just(TransactionScheduling::Standard),
        era_id_arb().prop_map(TransactionScheduling::FutureEra),
        timestamp_arb().prop_map(TransactionScheduling::FutureTimestamp),
    ]
}

pub fn transaction_invocation_target_arb() -> impl Strategy<Value = TransactionInvocationTarget> {
    prop_oneof![
        addressable_entity_hash_arb().prop_map(TransactionInvocationTarget::new_invocable_entity),
        Just(TransactionInvocationTarget::new_invocable_entity_alias(
            "abcd".to_string()
        )),
        Just(TransactionInvocationTarget::new_package_alias(
            "abcd".to_string(),
            None
        )),
        Just(TransactionInvocationTarget::new_package_alias(
            "abcd".to_string(),
            Some(10)
        )),
        u8_slice_32()
            .prop_map(|addr| { TransactionInvocationTarget::new_package(addr.into(), None) }),
        u8_slice_32()
            .prop_map(|addr| { TransactionInvocationTarget::new_package(addr.into(), Some(150)) }),
    ]
}

pub fn stored_transaction_target() -> impl Strategy<Value = TransactionTarget> {
    (
        transaction_invocation_target_arb(),
        transaction_stored_runtime_params_arb(),
    )
        .prop_map(|(id, runtime)| TransactionTarget::Stored { id, runtime })
}

fn transferred_value_arb() -> impl Strategy<Value = u64> {
    any::<u64>()
}

fn seed_arb() -> impl Strategy<Value = Option<[u8; 32]>> {
    option::of(array::uniform32(any::<u8>()))
}

pub fn session_transaction_target() -> impl Strategy<Value = TransactionTarget> {
    (
        any::<bool>(),
        Just(Bytes::from(vec![1; 10])),
        transaction_session_runtime_params_arb(),
    )
        .prop_map(
            |(is_install_upgrade, module_bytes, runtime)| TransactionTarget::Session {
                is_install_upgrade,
                module_bytes,
                runtime,
            },
        )
}

pub(crate) fn transaction_stored_runtime_params_arb(
) -> impl Strategy<Value = TransactionRuntimeParams> {
    prop_oneof![
        Just(TransactionRuntimeParams::VmCasperV1),
        transferred_value_arb().prop_map(|transferred_value| {
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value,
                seed: None,
            }
        }),
    ]
}

pub(crate) fn transaction_session_runtime_params_arb(
) -> impl Strategy<Value = TransactionRuntimeParams> {
    prop_oneof![
        Just(TransactionRuntimeParams::VmCasperV1),
        (transferred_value_arb(), seed_arb()).prop_map(|(transferred_value, seed)| {
            TransactionRuntimeParams::VmCasperV2 {
                transferred_value,
                seed,
            }
        })
    ]
}

pub fn transaction_target_arb() -> impl Strategy<Value = TransactionTarget> {
    prop_oneof![
        Just(TransactionTarget::Native),
        (
            transaction_invocation_target_arb(),
            transaction_stored_runtime_params_arb(),
        )
            .prop_map(|(id, runtime)| TransactionTarget::Stored { id, runtime }),
        (
            any::<bool>(),
            Just(Bytes::from(vec![1; 10])),
            transaction_session_runtime_params_arb(),
        )
            .prop_map(|(is_install_upgrade, module_bytes, runtime)| {
                TransactionTarget::Session {
                    is_install_upgrade,
                    module_bytes,
                    runtime,
                }
            })
    ]
}

pub fn legal_target_entry_point_calls_arb(
) -> impl Strategy<Value = (TransactionTarget, TransactionEntryPoint)> {
    prop_oneof![
        native_entry_point_arb().prop_map(|s| (TransactionTarget::Native, s)),
        stored_transaction_target()
            .prop_map(|s| (s, TransactionEntryPoint::Custom("ABC".to_string()))),
        session_transaction_target().prop_map(|s| (s, TransactionEntryPoint::Call)),
    ]
}

pub fn native_entry_point_arb() -> impl Strategy<Value = TransactionEntryPoint> {
    prop_oneof![
        Just(TransactionEntryPoint::Transfer),
        Just(TransactionEntryPoint::AddBid),
        Just(TransactionEntryPoint::WithdrawBid),
        Just(TransactionEntryPoint::Delegate),
        Just(TransactionEntryPoint::Undelegate),
        Just(TransactionEntryPoint::Redelegate),
        Just(TransactionEntryPoint::ActivateBid),
        Just(TransactionEntryPoint::ChangeBidPublicKey),
        Just(TransactionEntryPoint::AddReservations),
        Just(TransactionEntryPoint::CancelReservations),
    ]
}
pub fn transaction_entry_point_arb() -> impl Strategy<Value = TransactionEntryPoint> {
    prop_oneof![
        native_entry_point_arb(),
        Just(TransactionEntryPoint::Call),
        Just(TransactionEntryPoint::Custom("custom".to_string())),
    ]
}

pub fn runtime_args_arb() -> impl Strategy<Value = RuntimeArgs> {
    let mut runtime_args_1 = RuntimeArgs::new();
    let semi_random_string_pairs = [
        ("977837db-8dba-48c2-86f1-32f9740631db", "b7b3b3b3-8b3b-48c2-86f1-32f9740631db"),
        ("5de3eecc-b9c8-477f-bebe-937c3a16df85", "2ffd7939-34e5-4660-af9f-772a83011ce0"),
        ("036db036-8b7b-4009-a0d4-c9ce", "515f4fe6-06c8-45c5-8554-f07e727a842d036db036-8b7b-4009-a0d4-c9ce036db036-8b7b-4009-a0d4-c9ce"),
    ];
    for (key, val_str) in semi_random_string_pairs.iter() {
        let _ = runtime_args_1.insert(key.to_string(), Bytes::from(val_str.as_bytes()));
    }
    prop_oneof![Just(runtime_args_1)]
}

fn transaction_args_bytes_arbitrary() -> impl Strategy<Value = TransactionArgs> {
    prop::collection::vec(any::<u8>(), 0..100)
        .prop_map(|bytes| TransactionArgs::Bytesrepr(bytes.into()))
}

pub fn transaction_args_arb() -> impl Strategy<Value = TransactionArgs> {
    prop_oneof![
        runtime_args_arb().prop_map(TransactionArgs::Named),
        transaction_args_bytes_arbitrary()
    ]
}

pub fn fields_arb() -> impl Strategy<Value = BTreeMap<u16, Bytes>> {
    collection::btree_map(
        any::<u16>(),
        any::<String>().prop_map(|s| Bytes::from(s.as_bytes())),
        3..30,
    )
}
pub fn v1_transaction_payload_arb() -> impl Strategy<Value = TransactionV1Payload> {
    (
        any::<String>(),
        timestamp_arb(),
        any::<u64>(),
        pricing_mode_arb(),
        initiator_addr_arb(),
        fields_arb(),
    )
        .prop_map(
            |(chain_name, timestamp, ttl_millis, pricing_mode, initiator_addr, fields)| {
                TransactionV1Payload::new(
                    chain_name,
                    timestamp,
                    TimeDiff::from_millis(ttl_millis),
                    pricing_mode,
                    initiator_addr,
                    fields,
                )
            },
        )
}

pub fn fixed_pricing_mode_arb() -> impl Strategy<Value = PricingMode> {
    (any::<u8>(), any::<u8>()).prop_map(|(gas_price_tolerance, additional_computation_factor)| {
        PricingMode::Fixed {
            gas_price_tolerance,
            additional_computation_factor,
        }
    })
}

pub fn pricing_mode_arb() -> impl Strategy<Value = PricingMode> {
    prop_oneof![
        (any::<u64>(), any::<u8>(), any::<bool>()).prop_map(
            |(payment_amount, gas_price_tolerance, standard_payment)| {
                PricingMode::PaymentLimited {
                    payment_amount,
                    gas_price_tolerance,
                    standard_payment,
                }
            }
        ),
        fixed_pricing_mode_arb(),
    ]
}

pub fn initiator_addr_arb() -> impl Strategy<Value = InitiatorAddr> {
    prop_oneof![
        public_key_arb_no_system().prop_map(InitiatorAddr::PublicKey),
        u2_slice_32().prop_map(|hash| InitiatorAddr::AccountHash(AccountHash::new(hash))),
    ]
}

pub fn timestamp_arb() -> impl Strategy<Value = Timestamp> {
    //The weird u64 value is the max milliseconds that are bofeore year 10000. 5 digit years are
    // not rfc3339 compliant and will cause an error when trying to serialize to json.
    prop_oneof![Just(0_u64), Just(1_u64), Just(253_402_300_799_999_u64)].prop_map(Timestamp::from)
}

pub fn legal_v1_transaction_arb() -> impl Strategy<Value = TransactionV1> {
    (
        any::<String>(),
        timestamp_arb(),
        any::<u32>(),
        pricing_mode_arb(),
        secret_key_arb_no_system(),
        transaction_args_arb(),
        json_compliant_transaction_scheduling_arb(),
        legal_target_entry_point_calls_arb(),
    )
        .prop_map(
            |(
                chain_name,
                timestamp,
                ttl,
                pricing_mode,
                secret_key,
                args,
                scheduling,
                (target, entry_point),
            )| {
                let public_key = PublicKey::from(&secret_key);
                let initiator_addr = InitiatorAddr::PublicKey(public_key);
                let initiator_addr_with_secret = InitiatorAddrAndSecretKey::Both {
                    initiator_addr,
                    secret_key: &secret_key,
                };
                let container = FieldsContainer::new(args, target, entry_point, scheduling);
                TransactionV1::build(
                    chain_name,
                    timestamp,
                    TimeDiff::from_seconds(ttl),
                    pricing_mode,
                    container.to_map().unwrap(),
                    initiator_addr_with_secret,
                )
            },
        )
}
pub fn v1_transaction_arb() -> impl Strategy<Value = TransactionV1> {
    (
        any::<String>(),
        timestamp_arb(),
        any::<u32>(),
        pricing_mode_arb(),
        secret_key_arb_no_system(),
        runtime_args_arb(),
        transaction_target_arb(),
        transaction_entry_point_arb(),
        transaction_scheduling_arb(),
    )
        .prop_map(
            |(
                chain_name,
                timestamp,
                ttl,
                pricing_mode,
                secret_key,
                args,
                target,
                entry_point,
                scheduling,
            )| {
                let public_key = PublicKey::from(&secret_key);
                let initiator_addr = InitiatorAddr::PublicKey(public_key);
                let initiator_addr_with_secret = InitiatorAddrAndSecretKey::Both {
                    initiator_addr,
                    secret_key: &secret_key,
                };
                let container = FieldsContainer::new(
                    TransactionArgs::Named(args),
                    target,
                    entry_point,
                    scheduling,
                );
                TransactionV1::build(
                    chain_name,
                    timestamp,
                    TimeDiff::from_seconds(ttl),
                    pricing_mode,
                    container.to_map().unwrap(),
                    initiator_addr_with_secret,
                )
            },
        )
}

pub fn transaction_arb() -> impl Strategy<Value = Transaction> {
    (v1_transaction_arb()).prop_map(Transaction::V1)
}

pub fn legal_transaction_arb() -> impl Strategy<Value = Transaction> {
    (legal_v1_transaction_arb()).prop_map(Transaction::V1)
}
pub fn example_u32_arb() -> impl Strategy<Value = u32> {
    prop_oneof![Just(0), Just(1), Just(u32::MAX / 2), Just(u32::MAX)]
}
