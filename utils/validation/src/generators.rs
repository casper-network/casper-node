use std::{
    collections::{BTreeMap, BTreeSet},
    iter::FromIterator,
};

use casper_types::{
    account::{
        Account, AccountHash, ActionThresholds as AccountActionThresholds,
        AssociatedKeys as AccountAssociatedKeys, Weight as AccountWeight,
    },
    addressable_entity::{
        ActionThresholds, AddressableEntity, AssociatedKeys, EntityKind, MessageTopics, NamedKeys,
    },
    package::{EntityVersions, Groups, Package, PackageStatus},
    system::auction::{
        Bid, BidAddr, BidKind, Delegator, EraInfo, SeigniorageAllocation, UnbondingPurse,
        ValidatorBid, WithdrawPurse,
    },
    AccessRights, AddressableEntityHash, ByteCode, ByteCodeHash, ByteCodeKind, CLType, CLTyped,
    CLValue, DeployHash, DeployInfo, EntityVersionKey, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, EraId, Group, Key, PackageHash, Parameter, ProtocolVersion,
    PublicKey, SecretKey, StoredValue, Transfer, TransferAddr, URef, U512,
};
use casper_validation::{
    abi::{ABIFixture, ABITestCase},
    error::Error,
    Fixture, TestFixtures,
};

const DO_NOTHING_BYTES: &[u8] = b"\x00asm\x01\x00\x00\x00\x01\x04\x01`\x00\x00\x03\x02\x01\x00\x05\x03\x01\x00\x01\x07\x08\x01\x04call\x00\x00\n\x04\x01\x02\x00\x0b";

pub fn make_abi_test_fixtures() -> Result<TestFixtures, Error> {
    let basic_fixture = {
        let mut basic = BTreeMap::new();
        basic.insert(
            "SerializeU8".to_string(),
            ABITestCase::from_inputs(vec![254u8.into()])?,
        );
        basic.insert(
            "SerializeU16".to_string(),
            ABITestCase::from_inputs(vec![62356u16.into()])?,
        );
        basic.insert(
            "SerializeU32".to_string(),
            ABITestCase::from_inputs(vec![3490072870u32.into()])?,
        );
        basic.insert(
            "SerializeU64".to_string(),
            ABITestCase::from_inputs(vec![10829133186225377555u64.into()])?,
        );
        basic.insert(
            "SerializeEmptyString".to_string(),
            ABITestCase::from_inputs(vec![String::new().into()])?,
        );
        basic.insert(
            "SerializeString".to_string(),
            ABITestCase::from_inputs(vec!["Hello, world!".to_string().into()])?,
        );
        basic.insert(
            "SerializeBool".to_string(),
            ABITestCase::from_inputs(vec![true.into(), false.into()])?,
        );
        Fixture::ABI {
            name: "basic".to_string(),
            fixture: ABIFixture::from(basic),
        }
    };

    let transfer = Transfer::new(
        DeployHash::from_raw([44; 32]),
        AccountHash::new([100; 32]),
        Some(AccountHash::new([101; 32])),
        URef::new([10; 32], AccessRights::WRITE),
        URef::new([11; 32], AccessRights::WRITE),
        U512::from(15_000_000_000u64),
        U512::from(2_500_000_000u64),
        Some(1),
    );
    let deploy_info = DeployInfo::new(
        DeployHash::from_raw([55; 32]),
        &[TransferAddr::new([1; 32]), TransferAddr::new([2; 32])],
        AccountHash::new([100; 32]),
        URef::new([10; 32], AccessRights::READ_ADD_WRITE),
        U512::from(2_500_000_000u64),
    );

    let validator_secret_key =
        SecretKey::ed25519_from_bytes([42; 32]).expect("should create secret key");
    let delegator_secret_key =
        SecretKey::secp256k1_from_bytes([43; 32]).expect("should create secret key");

    let era_info = {
        let mut era_info = EraInfo::new();

        era_info
            .seigniorage_allocations_mut()
            .push(SeigniorageAllocation::Validator {
                validator_public_key: PublicKey::from(&validator_secret_key),
                amount: U512::from(1_000_000_000),
            });

        era_info
            .seigniorage_allocations_mut()
            .push(SeigniorageAllocation::Delegator {
                validator_public_key: PublicKey::from(&validator_secret_key),
                delegator_public_key: PublicKey::from(&delegator_secret_key),
                amount: U512::from(1_000_000_000),
            });
        era_info
    };

    let validator_public_key = PublicKey::from(&validator_secret_key);
    let validator_bid_key =
        Key::BidAddr(BidAddr::new_from_public_keys(&validator_public_key, None));
    let validator_bid = ValidatorBid::locked(
        validator_public_key.clone(),
        URef::new([10; 32], AccessRights::READ_ADD_WRITE),
        U512::from(50_000_000_000u64),
        100,
        u64::MAX,
    );
    let validator_bid_kind = BidKind::Validator(Box::new(validator_bid));
    let delegator_public_key = PublicKey::from(&delegator_secret_key);
    let delegator_bid_key = Key::BidAddr(BidAddr::new_from_public_keys(
        &validator_public_key,
        Some(&delegator_public_key),
    ));
    let delegator_bid = Delegator::locked(
        delegator_public_key,
        U512::from(1_000_000_000u64),
        URef::new([11; 32], AccessRights::READ_ADD_WRITE),
        validator_public_key.clone(),
        u64::MAX,
    );
    let delegator_bid_kind = BidKind::Delegator(Box::new(delegator_bid.clone()));

    let unified_bid_key = Key::BidAddr(BidAddr::legacy(
        validator_public_key.to_account_hash().value(),
    ));
    let unified_bid = {
        let mut unified_bid = Bid::locked(
            validator_public_key.clone(),
            URef::new([10; 32], AccessRights::READ_ADD_WRITE),
            U512::from(50_000_000_000u64),
            100,
            u64::MAX,
        );
        unified_bid.delegators_mut().insert(
            delegator_bid.delegator_public_key().clone(),
            delegator_bid.clone(),
        );
        unified_bid
    };
    let unified_bid_kind = BidKind::Unified(Box::new(unified_bid));

    let original_bid_key = Key::Bid(validator_public_key.to_account_hash());
    let original_bid = {
        let mut bid = Bid::locked(
            validator_public_key,
            URef::new([10; 32], AccessRights::READ_ADD_WRITE),
            U512::from(50_000_000_000u64),
            100,
            u64::MAX,
        );
        bid.delegators_mut()
            .insert(delegator_bid.delegator_public_key().clone(), delegator_bid);
        bid
    };

    let withdraw_purse_1 = WithdrawPurse::new(
        URef::new([10; 32], AccessRights::READ),
        PublicKey::from(&validator_secret_key),
        PublicKey::from(&validator_secret_key),
        EraId::new(41),
        U512::from(60_000_000_000u64),
    );
    let withdraw_purse_2 = WithdrawPurse::new(
        URef::new([11; 32], AccessRights::READ),
        PublicKey::from(&validator_secret_key),
        PublicKey::from(&delegator_secret_key),
        EraId::new(42),
        U512::from(50_000_000_000u64),
    );
    let unbonding_purse_1 = UnbondingPurse::new(
        URef::new([10; 32], AccessRights::READ),
        PublicKey::from(&validator_secret_key),
        PublicKey::from(&validator_secret_key),
        EraId::new(41),
        U512::from(60_000_000_000u64),
        None,
    );
    let unbonding_purse_2 = UnbondingPurse::new(
        URef::new([11; 32], AccessRights::READ),
        PublicKey::from(&validator_secret_key),
        PublicKey::from(&delegator_secret_key),
        EraId::new(42),
        U512::from(50_000_000_000u64),
        None,
    );

    let keys_fixture = {
        const ACCOUNT_KEY: Key = Key::Account(AccountHash::new([42; 32]));
        const HASH_KEY: Key = Key::Hash([42; 32]);
        const UREF_KEY: Key = Key::URef(URef::new([42; 32], AccessRights::READ));
        const TRANSFER_KEY: Key = Key::Transfer(TransferAddr::new([42; 32]));
        const DEPLOY_INFO_KEY: Key = Key::DeployInfo(DeployHash::from_raw([42; 32]));
        const ERA_INFO_KEY: Key = Key::EraInfo(EraId::new(42));
        const BALANCE_KEY: Key = Key::Balance([42; 32]);
        const WITHDRAW_KEY: Key = Key::Withdraw(AccountHash::new([42; 32]));
        const DICTIONARY_KEY: Key = Key::Dictionary([42; 32]);
        const SYSTEM_CONTRACT_REGISTRY_KEY: Key = Key::SystemEntityRegistry;
        const ERA_SUMMARY_KEY: Key = Key::EraSummary;
        const UNBOND_KEY: Key = Key::Unbond(AccountHash::new([42; 32]));
        const CHAINSPEC_REGISTRY_KEY: Key = Key::ChainspecRegistry;
        const CHECKSUM_REGISTRY_KEY: Key = Key::ChecksumRegistry;

        let mut keys = BTreeMap::new();
        keys.insert(
            "Account".to_string(),
            ABITestCase::from_inputs(vec![ACCOUNT_KEY.into()])?,
        );
        keys.insert(
            "Hash".to_string(),
            ABITestCase::from_inputs(vec![HASH_KEY.into()])?,
        );
        keys.insert(
            "URef".to_string(),
            ABITestCase::from_inputs(vec![UREF_KEY.into()])?,
        );
        keys.insert(
            "Transfer".to_string(),
            ABITestCase::from_inputs(vec![TRANSFER_KEY.into()])?,
        );
        keys.insert(
            "DeployInfo".to_string(),
            ABITestCase::from_inputs(vec![DEPLOY_INFO_KEY.into()])?,
        );
        keys.insert(
            "EraInfo".to_string(),
            ABITestCase::from_inputs(vec![ERA_INFO_KEY.into()])?,
        );
        keys.insert(
            "Balance".to_string(),
            ABITestCase::from_inputs(vec![BALANCE_KEY.into()])?,
        );
        keys.insert(
            "WriteBid".to_string(),
            ABITestCase::from_inputs(vec![original_bid_key.into()])?,
        );
        keys.insert(
            "WriteUnifiedBid".to_string(),
            ABITestCase::from_inputs(vec![unified_bid_key.into()])?,
        );
        keys.insert(
            "WriteValidatorBid".to_string(),
            ABITestCase::from_inputs(vec![validator_bid_key.into()])?,
        );
        keys.insert(
            "WriteDelegatorBid".to_string(),
            ABITestCase::from_inputs(vec![delegator_bid_key.into()])?,
        );

        keys.insert(
            "Withdraw".to_string(),
            ABITestCase::from_inputs(vec![WITHDRAW_KEY.into()])?,
        );
        keys.insert(
            "Dictionary".to_string(),
            ABITestCase::from_inputs(vec![DICTIONARY_KEY.into()])?,
        );
        keys.insert(
            "SystemContractRegistry".to_string(),
            ABITestCase::from_inputs(vec![SYSTEM_CONTRACT_REGISTRY_KEY.into()])?,
        );
        keys.insert(
            "EraSummary".to_string(),
            ABITestCase::from_inputs(vec![ERA_SUMMARY_KEY.into()])?,
        );
        keys.insert(
            "Unbond".to_string(),
            ABITestCase::from_inputs(vec![UNBOND_KEY.into()])?,
        );
        keys.insert(
            "ChainspecRegistry".to_string(),
            ABITestCase::from_inputs(vec![CHAINSPEC_REGISTRY_KEY.into()])?,
        );
        keys.insert(
            "ChecksumRegistry".to_string(),
            ABITestCase::from_inputs(vec![CHECKSUM_REGISTRY_KEY.into()])?,
        );
        Fixture::ABI {
            name: "key".to_string(),
            fixture: ABIFixture::from(keys),
        }
    };

    let stored_value_fixture = {
        let mut stored_value = BTreeMap::new();

        let cl_value = CLValue::from_t("Hello, world!").expect("should create cl value");

        stored_value.insert(
            "CLValue".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::CLValue(cl_value).into()])?,
        );

        let account_secret_key =
            SecretKey::ed25519_from_bytes([42; 32]).expect("should create secret key");
        let account_public_key = PublicKey::from(&account_secret_key);
        let account_hash = account_public_key.to_account_hash();

        let account_named_keys = {
            let mut named_keys = NamedKeys::new();
            named_keys.insert("hash".to_string(), Key::Hash([42; 32]));
            named_keys.insert(
                "uref".to_string(),
                Key::URef(URef::new([16; 32], AccessRights::READ_ADD_WRITE)),
            );
            named_keys
        };

        let associated_keys = AccountAssociatedKeys::new(account_hash, AccountWeight::new(1));

        let account = Account::new(
            account_hash,
            account_named_keys,
            URef::new([17; 32], AccessRights::WRITE),
            associated_keys,
            AccountActionThresholds::new(AccountWeight::new(1), AccountWeight::new(1)).unwrap(),
        );

        stored_value.insert(
            "Account".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::Account(account).into()])?,
        );

        let byte_code = ByteCode::new(ByteCodeKind::V1CasperWasm, DO_NOTHING_BYTES.to_vec());

        stored_value.insert(
            "ByteCode".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::ByteCode(byte_code).into()])?,
        );

        let entry_points = {
            let mut entry_points = EntryPoints::new();
            let public_contract_entry_point = EntryPoint::new(
                "public_entry_point_func",
                vec![
                    Parameter::new("param1", U512::cl_type()),
                    Parameter::new("param2", String::cl_type()),
                ],
                CLType::Unit,
                EntryPointAccess::Public,
                EntryPointType::AddressableEntity,
            );

            entry_points.add_entry_point(public_contract_entry_point);

            entry_points
        };

        let entity = AddressableEntity::new(
            PackageHash::new([100; 32]),
            ByteCodeHash::new([101; 32]),
            entry_points,
            ProtocolVersion::V1_0_0,
            URef::default(),
            AssociatedKeys::default(),
            ActionThresholds::default(),
            MessageTopics::default(),
            EntityKind::SmartContract,
        );
        stored_value.insert(
            "AddressableEntity".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::AddressableEntity(entity).into()])?,
        );

        let mut active_versions = BTreeMap::new();
        let v1_hash = AddressableEntityHash::new([99; 32]);
        let v2_hash = AddressableEntityHash::new([100; 32]);
        active_versions.insert(EntityVersionKey::new(1, 2), v1_hash);
        let v1 = EntityVersionKey::new(1, 1);
        active_versions.insert(v1, v2_hash);
        let active_versions = EntityVersions::from(active_versions);

        let mut disabled_versions = BTreeSet::new();
        disabled_versions.insert(v1);

        let mut groups = Groups::new();
        groups.insert(Group::new("Empty"), BTreeSet::new());
        groups.insert(
            Group::new("Single"),
            BTreeSet::from_iter(vec![URef::new([55; 32], AccessRights::READ)]),
        );

        let package = Package::new(
            URef::new([39; 32], AccessRights::READ),
            active_versions,
            disabled_versions,
            groups,
            PackageStatus::Locked,
        );

        stored_value.insert(
            "Package".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::Package(package).into()])?,
        );

        stored_value.insert(
            "Transfer".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::Transfer(transfer).into()])?,
        );
        stored_value.insert(
            "DeployInfo".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::DeployInfo(deploy_info).into()])?,
        );
        stored_value.insert(
            "EraInfo".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::EraInfo(era_info).into()])?,
        );

        stored_value.insert(
            "Bid".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::Bid(Box::new(original_bid)).into()])?,
        );
        stored_value.insert(
            "UnifiedBid".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::BidKind(unified_bid_kind).into()])?,
        );
        stored_value.insert(
            "ValidatorBid".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::BidKind(validator_bid_kind).into()])?,
        );
        stored_value.insert(
            "DelegatorBid".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::BidKind(delegator_bid_kind).into()])?,
        );
        stored_value.insert(
            "Withdraw".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::Withdraw(vec![
                withdraw_purse_1,
                withdraw_purse_2,
            ])
            .into()])?,
        );
        stored_value.insert(
            "Unbonding".to_string(),
            ABITestCase::from_inputs(vec![StoredValue::Unbonding(vec![
                unbonding_purse_1,
                unbonding_purse_2,
            ])
            .into()])?,
        );

        Fixture::ABI {
            name: "stored_value".to_string(),
            fixture: ABIFixture::from(stored_value),
        }
    };

    Ok(vec![basic_fixture, stored_value_fixture, keys_fixture])
}
