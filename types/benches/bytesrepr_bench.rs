use criterion::{black_box, criterion_group, criterion_main, Bencher, Criterion};

use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
};

use casper_types::{
    account::{Account, AccountHash, ActionThresholds, AssociatedKeys, Weight},
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    contracts::{ContractPackageStatus, NamedKeys},
    system::auction::{Bid, Delegator, EraInfo, SeigniorageAllocation},
    AccessRights, CLType, CLTyped, CLValue, Contract, ContractHash, ContractPackage,
    ContractPackageHash, ContractVersionKey, ContractWasmHash, DeployHash, DeployInfo, EntryPoint,
    EntryPointAccess, EntryPointType, EntryPoints, Group, Key, Parameter, ProtocolVersion,
    PublicKey, SecretKey, Transfer, TransferAddr, URef, KEY_HASH_LENGTH, TRANSFER_ADDR_LENGTH,
    U128, U256, U512, UREF_ADDR_LENGTH,
};

static KB: usize = 1024;
static BATCH: usize = 4 * KB;

const TEST_I32: i32 = 123_456_789;
const TEST_U128: U128 = U128([123_456_789, 0]);
const TEST_U256: U256 = U256([123_456_789, 0, 0, 0]);
const TEST_U512: U512 = U512([123_456_789, 0, 0, 0, 0, 0, 0, 0]);
const TEST_STR_1: &str = "String One";
const TEST_STR_2: &str = "String Two";

fn prepare_vector(size: usize) -> Vec<i32> {
    (0..size as i32).collect()
}

fn serialize_vector_of_i32s(b: &mut Bencher) {
    let data = prepare_vector(black_box(BATCH));
    b.iter(|| data.to_bytes());
}

fn deserialize_vector_of_i32s(b: &mut Bencher) {
    let data = prepare_vector(black_box(BATCH)).to_bytes().unwrap();
    b.iter(|| {
        let (res, _rem): (Vec<i32>, _) = FromBytes::from_bytes(&data).unwrap();
        res
    });
}

fn serialize_vector_of_u8(b: &mut Bencher) {
    // 0, 1, ... 254, 255, 0, 1, ...
    let data: Bytes = prepare_vector(BATCH)
        .into_iter()
        .map(|value| value as u8)
        .collect();
    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

fn deserialize_vector_of_u8(b: &mut Bencher) {
    // 0, 1, ... 254, 255, 0, 1, ...
    let data: Vec<u8> = prepare_vector(BATCH)
        .into_iter()
        .map(|value| value as u8)
        .collect::<Bytes>()
        .to_bytes()
        .unwrap();
    b.iter(|| Bytes::from_bytes(black_box(&data)))
}

fn serialize_u8(b: &mut Bencher) {
    b.iter(|| ToBytes::to_bytes(black_box(&129u8)));
}

fn deserialize_u8(b: &mut Bencher) {
    b.iter(|| u8::from_bytes(black_box(&[129u8])));
}

fn serialize_i32(b: &mut Bencher) {
    b.iter(|| ToBytes::to_bytes(black_box(&1_816_142_132i32)));
}

fn deserialize_i32(b: &mut Bencher) {
    b.iter(|| i32::from_bytes(black_box(&[0x34, 0x21, 0x40, 0x6c])));
}

fn serialize_u64(b: &mut Bencher) {
    b.iter(|| ToBytes::to_bytes(black_box(&14_157_907_845_468_752_670u64)));
}

fn deserialize_u64(b: &mut Bencher) {
    b.iter(|| u64::from_bytes(black_box(&[0x1e, 0x8b, 0xe1, 0x73, 0x2c, 0xfe, 0x7a, 0xc4])));
}

fn serialize_some_u64(b: &mut Bencher) {
    let data = Some(14_157_907_845_468_752_670u64);

    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

fn deserialize_some_u64(b: &mut Bencher) {
    let data = Some(14_157_907_845_468_752_670u64);
    let data = data.to_bytes().unwrap();

    b.iter(|| Option::<u64>::from_bytes(&data));
}

fn serialize_none_u64(b: &mut Bencher) {
    let data: Option<u64> = None;

    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

fn deserialize_ok_u64(b: &mut Bencher) {
    let data: Option<u64> = None;
    let data = data.to_bytes().unwrap();
    b.iter(|| Option::<u64>::from_bytes(&data));
}

fn make_test_vec_of_vec8() -> Vec<Bytes> {
    (0..4)
        .map(|_v| {
            // 0, 1, 2, ..., 254, 255
            let inner_vec = iter::repeat_with(|| 0..255u8)
                .flatten()
                // 4 times to create 4x 1024 bytes
                .take(4)
                .collect::<Vec<_>>();
            Bytes::from(inner_vec)
        })
        .collect()
}

fn serialize_vector_of_vector_of_u8(b: &mut Bencher) {
    let data = make_test_vec_of_vec8();
    b.iter(|| data.to_bytes());
}

fn deserialize_vector_of_vector_of_u8(b: &mut Bencher) {
    let data = make_test_vec_of_vec8().to_bytes().unwrap();
    b.iter(|| Vec::<Bytes>::from_bytes(black_box(&data)));
}

fn serialize_tree_map(b: &mut Bencher) {
    let data = {
        let mut res = BTreeMap::new();
        res.insert("asdf".to_string(), "zxcv".to_string());
        res.insert("qwer".to_string(), "rewq".to_string());
        res.insert("1234".to_string(), "5678".to_string());
        res
    };

    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

fn deserialize_treemap(b: &mut Bencher) {
    let data = {
        let mut res = BTreeMap::new();
        res.insert("asdf".to_string(), "zxcv".to_string());
        res.insert("qwer".to_string(), "rewq".to_string());
        res.insert("1234".to_string(), "5678".to_string());
        res
    };
    let data = data.to_bytes().unwrap();
    b.iter(|| BTreeMap::<String, String>::from_bytes(black_box(&data)));
}

fn serialize_string(b: &mut Bencher) {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let data = lorem.to_string();
    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

fn deserialize_string(b: &mut Bencher) {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let data = lorem.to_bytes().unwrap();
    b.iter(|| String::from_bytes(&data));
}

fn serialize_vec_of_string(b: &mut Bencher) {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.".to_string();
    let array_of_lorem: Vec<String> = lorem.split(' ').map(Into::into).collect();
    let data = array_of_lorem;
    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

fn deserialize_vec_of_string(b: &mut Bencher) {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.".to_string();
    let array_of_lorem: Vec<String> = lorem.split(' ').map(Into::into).collect();
    let data = array_of_lorem.to_bytes().unwrap();

    b.iter(|| Vec::<String>::from_bytes(&data));
}

fn serialize_unit(b: &mut Bencher) {
    b.iter(|| ToBytes::to_bytes(black_box(&())))
}

fn deserialize_unit(b: &mut Bencher) {
    let data = ().to_bytes().unwrap();

    b.iter(|| <()>::from_bytes(&data))
}

fn serialize_key_account(b: &mut Bencher) {
    let account = Key::Account(AccountHash::new([0u8; 32]));

    b.iter(|| ToBytes::to_bytes(black_box(&account)))
}

fn deserialize_key_account(b: &mut Bencher) {
    let account = Key::Account(AccountHash::new([0u8; 32]));
    let account_bytes = account.to_bytes().unwrap();

    b.iter(|| Key::from_bytes(black_box(&account_bytes)))
}

fn serialize_key_hash(b: &mut Bencher) {
    let hash = Key::Hash([0u8; 32]);
    b.iter(|| ToBytes::to_bytes(black_box(&hash)))
}

fn deserialize_key_hash(b: &mut Bencher) {
    let hash = Key::Hash([0u8; 32]);
    let hash_bytes = hash.to_bytes().unwrap();

    b.iter(|| Key::from_bytes(black_box(&hash_bytes)))
}

fn serialize_key_uref(b: &mut Bencher) {
    let uref = Key::URef(URef::new([0u8; 32], AccessRights::ADD_WRITE));
    b.iter(|| ToBytes::to_bytes(black_box(&uref)))
}

fn deserialize_key_uref(b: &mut Bencher) {
    let uref = Key::URef(URef::new([0u8; 32], AccessRights::ADD_WRITE));
    let uref_bytes = uref.to_bytes().unwrap();

    b.iter(|| Key::from_bytes(black_box(&uref_bytes)))
}

fn serialize_vec_of_keys(b: &mut Bencher) {
    let keys: Vec<Key> = (0..32)
        .map(|i| Key::URef(URef::new([i; 32], AccessRights::ADD_WRITE)))
        .collect();
    b.iter(|| ToBytes::to_bytes(black_box(&keys)))
}

fn deserialize_vec_of_keys(b: &mut Bencher) {
    let keys: Vec<Key> = (0..32)
        .map(|i| Key::URef(URef::new([i; 32], AccessRights::ADD_WRITE)))
        .collect();
    let keys_bytes = keys.to_bytes().unwrap();
    b.iter(|| Vec::<Key>::from_bytes(black_box(&keys_bytes)));
}

fn serialize_access_rights_read(b: &mut Bencher) {
    b.iter(|| AccessRights::READ.to_bytes());
}

fn deserialize_access_rights_read(b: &mut Bencher) {
    let data = AccessRights::READ.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

fn serialize_access_rights_write(b: &mut Bencher) {
    b.iter(|| AccessRights::WRITE.to_bytes());
}

fn deserialize_access_rights_write(b: &mut Bencher) {
    let data = AccessRights::WRITE.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

fn serialize_access_rights_add(b: &mut Bencher) {
    b.iter(|| AccessRights::ADD.to_bytes());
}

fn deserialize_access_rights_add(b: &mut Bencher) {
    let data = AccessRights::ADD.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

fn serialize_access_rights_read_add(b: &mut Bencher) {
    b.iter(|| AccessRights::READ_ADD.to_bytes());
}

fn deserialize_access_rights_read_add(b: &mut Bencher) {
    let data = AccessRights::READ_ADD.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

fn serialize_access_rights_read_write(b: &mut Bencher) {
    b.iter(|| AccessRights::READ_WRITE.to_bytes());
}

fn deserialize_access_rights_read_write(b: &mut Bencher) {
    let data = AccessRights::READ_WRITE.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

fn serialize_access_rights_add_write(b: &mut Bencher) {
    b.iter(|| AccessRights::ADD_WRITE.to_bytes());
}

fn deserialize_access_rights_add_write(b: &mut Bencher) {
    let data = AccessRights::ADD_WRITE.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

fn serialize_cl_value<T: CLTyped + ToBytes>(raw_value: T) -> Vec<u8> {
    CLValue::from_t(raw_value)
        .expect("should create CLValue")
        .to_bytes()
        .expect("should serialize CLValue")
}

fn benchmark_deserialization<T: CLTyped + ToBytes + FromBytes>(b: &mut Bencher, raw_value: T) {
    let serialized_value = serialize_cl_value(raw_value);
    b.iter(|| {
        let cl_value: CLValue = bytesrepr::deserialize_from_slice(&serialized_value).unwrap();
        let _raw_value: T = cl_value.into_t().unwrap();
    });
}

fn serialize_cl_value_int32(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_I32));
}

fn deserialize_cl_value_int32(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_I32);
}

fn serialize_cl_value_uint128(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_U128));
}

fn deserialize_cl_value_uint128(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_U128);
}

fn serialize_cl_value_uint256(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_U256));
}

fn deserialize_cl_value_uint256(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_U256);
}

fn serialize_cl_value_uint512(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_U512));
}

fn deserialize_cl_value_uint512(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_U512);
}

fn serialize_cl_value_bytearray(b: &mut Bencher) {
    b.iter_with_setup(
        || {
            let vec: Vec<u8> = (0..255).collect();
            Bytes::from(vec)
        },
        serialize_cl_value,
    );
}

fn deserialize_cl_value_bytearray(b: &mut Bencher) {
    let vec = (0..255).collect::<Vec<u8>>();
    let bytes: Bytes = vec.into();
    benchmark_deserialization(b, bytes);
}

fn serialize_cl_value_listint32(b: &mut Bencher) {
    b.iter(|| serialize_cl_value((0..1024).collect::<Vec<i32>>()));
}

fn deserialize_cl_value_listint32(b: &mut Bencher) {
    benchmark_deserialization(b, (0..1024).collect::<Vec<i32>>());
}

fn serialize_cl_value_string(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_STR_1.to_string()));
}

fn deserialize_cl_value_string(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_STR_1.to_string());
}

fn serialize_cl_value_liststring(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(vec![TEST_STR_1.to_string(), TEST_STR_2.to_string()]));
}

fn deserialize_cl_value_liststring(b: &mut Bencher) {
    benchmark_deserialization(b, vec![TEST_STR_1.to_string(), TEST_STR_2.to_string()]);
}

fn serialize_cl_value_namedkey(b: &mut Bencher) {
    b.iter(|| {
        serialize_cl_value((
            TEST_STR_1.to_string(),
            Key::Account(AccountHash::new([0xffu8; 32])),
        ))
    });
}

fn deserialize_cl_value_namedkey(b: &mut Bencher) {
    benchmark_deserialization(
        b,
        (
            TEST_STR_1.to_string(),
            Key::Account(AccountHash::new([0xffu8; 32])),
        ),
    );
}

fn serialize_u128(b: &mut Bencher) {
    let num_u128 = U128::default();
    b.iter(|| ToBytes::to_bytes(black_box(&num_u128)))
}

fn deserialize_u128(b: &mut Bencher) {
    let num_u128 = U128::default();
    let num_u128_bytes = num_u128.to_bytes().unwrap();

    b.iter(|| U128::from_bytes(black_box(&num_u128_bytes)))
}

fn serialize_u256(b: &mut Bencher) {
    let num_u256 = U256::default();
    b.iter(|| ToBytes::to_bytes(black_box(&num_u256)))
}

fn deserialize_u256(b: &mut Bencher) {
    let num_u256 = U256::default();
    let num_u256_bytes = num_u256.to_bytes().unwrap();

    b.iter(|| U256::from_bytes(black_box(&num_u256_bytes)))
}

fn serialize_u512(b: &mut Bencher) {
    let num_u512 = U512::default();
    b.iter(|| ToBytes::to_bytes(black_box(&num_u512)))
}

fn deserialize_u512(b: &mut Bencher) {
    let num_u512 = U512::default();
    let num_u512_bytes = num_u512.to_bytes().unwrap();

    b.iter(|| U512::from_bytes(black_box(&num_u512_bytes)))
}

fn sample_account(associated_keys_len: u8, named_keys_len: u8) -> Account {
    let account_hash = AccountHash::default();
    let named_keys: NamedKeys = sample_named_keys(named_keys_len);
    let main_purse = URef::default();
    let associated_keys = {
        let mut tmp = AssociatedKeys::new(AccountHash::default(), Weight::new(1));
        (1..associated_keys_len).for_each(|i| {
            tmp.add_key(
                AccountHash::new([i; casper_types::account::ACCOUNT_HASH_LENGTH]),
                Weight::new(1),
            )
            .unwrap()
        });
        tmp
    };
    let action_thresholds = ActionThresholds::default();
    Account::new(
        account_hash,
        named_keys,
        main_purse,
        associated_keys,
        action_thresholds,
    )
}

fn serialize_account(b: &mut Bencher) {
    let account = sample_account(10, 10);
    b.iter(|| ToBytes::to_bytes(black_box(&account)));
}

fn deserialize_account(b: &mut Bencher) {
    let account = sample_account(10, 10);
    let account_bytes = Account::to_bytes(&account).unwrap();
    b.iter(|| Account::from_bytes(black_box(&account_bytes)).unwrap());
}

fn serialize_contract(b: &mut Bencher) {
    let contract = sample_contract(10, 10);
    b.iter(|| ToBytes::to_bytes(black_box(&contract)));
}

fn deserialize_contract(b: &mut Bencher) {
    let contract = sample_contract(10, 10);
    let contract_bytes = Contract::to_bytes(&contract).unwrap();
    b.iter(|| Contract::from_bytes(black_box(&contract_bytes)).unwrap());
}

fn sample_named_keys(len: u8) -> BTreeMap<String, Key> {
    (0..len)
        .map(|i| {
            (
                format!("named-key-{}", i),
                Key::Account(AccountHash::default()),
            )
        })
        .collect()
}

fn sample_contract(named_keys_len: u8, entry_points_len: u8) -> Contract {
    let named_keys: NamedKeys = sample_named_keys(named_keys_len);

    let entry_points = {
        let mut tmp = EntryPoints::default();
        (1..entry_points_len).for_each(|i| {
            let args = vec![
                Parameter::new("first", CLType::U32),
                Parameter::new("Foo", CLType::U32),
            ];
            let entry_point = EntryPoint::new(
                format!("test-{}", i),
                args,
                casper_types::CLType::U512,
                EntryPointAccess::groups(&["Group 2"]),
                EntryPointType::Contract,
            );
            tmp.add_entry_point(entry_point);
        });
        tmp
    };

    casper_types::contracts::Contract::new(
        ContractPackageHash::default(),
        ContractWasmHash::default(),
        named_keys,
        entry_points,
        ProtocolVersion::default(),
    )
}

fn contract_version_key_fn(i: u8) -> ContractVersionKey {
    ContractVersionKey::new(i as u32, i as u32)
}

fn contract_hash_fn(i: u8) -> ContractHash {
    ContractHash::new([i; KEY_HASH_LENGTH])
}

fn sample_map<K: Ord, V, FK, FV>(key_fn: FK, value_fn: FV, count: u8) -> BTreeMap<K, V>
where
    FK: Fn(u8) -> K,
    FV: Fn(u8) -> V,
{
    (0..count)
        .map(|i| {
            let key = key_fn(i);
            let value = value_fn(i);
            (key, value)
        })
        .collect()
}

fn sample_set<K: Ord, F>(fun: F, count: u8) -> BTreeSet<K>
where
    F: Fn(u8) -> K,
{
    (0..count).map(fun).collect()
}

fn sample_group(i: u8) -> Group {
    Group::new(format!("group-{}", i))
}

fn sample_uref(i: u8) -> URef {
    URef::new([i; UREF_ADDR_LENGTH], AccessRights::all())
}

fn sample_contract_package(
    contract_versions_len: u8,
    disabled_versions_len: u8,
    groups_len: u8,
) -> ContractPackage {
    let access_key = URef::default();
    let versions = sample_map(
        contract_version_key_fn,
        contract_hash_fn,
        contract_versions_len,
    );
    let disabled_versions = sample_set(contract_version_key_fn, disabled_versions_len);
    let groups = sample_map(sample_group, |_| sample_set(sample_uref, 3), groups_len);

    ContractPackage::new(
        access_key,
        versions,
        disabled_versions,
        groups,
        ContractPackageStatus::Locked,
    )
}

fn serialize_contract_package(b: &mut Bencher) {
    let contract = sample_contract_package(5, 1, 5);
    b.iter(|| ContractPackage::to_bytes(black_box(&contract)));
}

fn deserialize_contract_package(b: &mut Bencher) {
    let contract_package = sample_contract_package(5, 1, 5);
    let contract_bytes = ContractPackage::to_bytes(&contract_package).unwrap();
    b.iter(|| ContractPackage::from_bytes(black_box(&contract_bytes)).unwrap());
}

fn u32_to_pk(i: u32) -> PublicKey {
    let mut sk_bytes = [0u8; 32];
    U256::from(i).to_big_endian(&mut sk_bytes);
    let sk = SecretKey::ed25519_from_bytes(sk_bytes).unwrap();
    PublicKey::from(&sk)
}

fn sample_delegators(delegators_len: u32) -> Vec<Delegator> {
    (0..delegators_len)
        .map(|i| {
            let delegator_pk = u32_to_pk(i);
            let staked_amount = U512::from_dec_str("123123123123123").unwrap();
            let bonding_purse = URef::default();
            let validator_pk = u32_to_pk(i);
            Delegator::unlocked(delegator_pk, staked_amount, bonding_purse, validator_pk)
        })
        .collect()
}

fn sample_bid(delegators_len: u32) -> Bid {
    let validator_public_key = PublicKey::System;
    let bonding_purse = URef::default();
    let staked_amount = U512::from_dec_str("123123123123123").unwrap();
    let delegation_rate = 10u8;
    let mut bid = Bid::unlocked(
        validator_public_key,
        bonding_purse,
        staked_amount,
        delegation_rate,
    );
    let new_delegators = sample_delegators(delegators_len);

    let curr_delegators = bid.delegators_mut();
    for delegator in new_delegators.into_iter() {
        assert!(curr_delegators
            .insert(delegator.delegator_public_key().clone(), delegator)
            .is_none());
    }
    bid
}

fn serialize_bid(delegators_len: u32, b: &mut Bencher) {
    let bid = sample_bid(delegators_len);
    b.iter(|| Bid::to_bytes(black_box(&bid)));
}

fn deserialize_bid(delegators_len: u32, b: &mut Bencher) {
    let bid = sample_bid(delegators_len);
    let bid_bytes = Bid::to_bytes(&bid).unwrap();
    b.iter(|| Bid::from_bytes(black_box(&bid_bytes)));
}

fn sample_transfer() -> Transfer {
    Transfer::new(
        DeployHash::default(),
        AccountHash::default(),
        None,
        URef::default(),
        URef::default(),
        U512::MAX,
        U512::from_dec_str("123123123123").unwrap(),
        Some(1u64),
    )
}

fn serialize_transfer(b: &mut Bencher) {
    let transfer = sample_transfer();
    b.iter(|| Transfer::to_bytes(&transfer));
}

fn deserialize_transfer(b: &mut Bencher) {
    let transfer = sample_transfer();
    let transfer_bytes = transfer.to_bytes().unwrap();
    b.iter(|| Transfer::from_bytes(&transfer_bytes));
}

fn sample_deploy_info(transfer_len: u16) -> DeployInfo {
    let transfers = (0..transfer_len)
        .map(|i| {
            let mut tmp = [0u8; TRANSFER_ADDR_LENGTH];
            U256::from(i).to_little_endian(&mut tmp);
            TransferAddr::new(tmp)
        })
        .collect::<Vec<_>>();
    DeployInfo::new(
        DeployHash::default(),
        &transfers,
        AccountHash::default(),
        URef::default(),
        U512::MAX,
    )
}

fn serialize_deploy_info(b: &mut Bencher) {
    let deploy_info = sample_deploy_info(1000);
    b.iter(|| DeployInfo::to_bytes(&deploy_info));
}

fn deserialize_deploy_info(b: &mut Bencher) {
    let deploy_info = sample_deploy_info(1000);
    let deploy_bytes = deploy_info.to_bytes().unwrap();
    b.iter(|| DeployInfo::from_bytes(&deploy_bytes));
}

fn sample_era_info(delegators_len: u32) -> EraInfo {
    let mut base = EraInfo::new();
    let delegations = (0..delegators_len).map(|i| {
        let pk = u32_to_pk(i);
        SeigniorageAllocation::delegator(pk.clone(), pk, U512::MAX)
    });
    base.seigniorage_allocations_mut().extend(delegations);
    base
}

fn serialize_era_info(delegators_len: u32, b: &mut Bencher) {
    let era_info = sample_era_info(delegators_len);
    b.iter(|| EraInfo::to_bytes(&era_info));
}

fn deserialize_era_info(delegators_len: u32, b: &mut Bencher) {
    let era_info = sample_era_info(delegators_len);
    let era_info_bytes = era_info.to_bytes().unwrap();
    b.iter(|| EraInfo::from_bytes(&era_info_bytes));
}

fn bytesrepr_bench(c: &mut Criterion) {
    c.bench_function("serialize_vector_of_i32s", serialize_vector_of_i32s);
    c.bench_function("deserialize_vector_of_i32s", deserialize_vector_of_i32s);
    c.bench_function("serialize_vector_of_u8", serialize_vector_of_u8);
    c.bench_function("deserialize_vector_of_u8", deserialize_vector_of_u8);
    c.bench_function("serialize_u8", serialize_u8);
    c.bench_function("deserialize_u8", deserialize_u8);
    c.bench_function("serialize_i32", serialize_i32);
    c.bench_function("deserialize_i32", deserialize_i32);
    c.bench_function("serialize_u64", serialize_u64);
    c.bench_function("deserialize_u64", deserialize_u64);
    c.bench_function("serialize_some_u64", serialize_some_u64);
    c.bench_function("deserialize_some_u64", deserialize_some_u64);
    c.bench_function("serialize_none_u64", serialize_none_u64);
    c.bench_function("deserialize_ok_u64", deserialize_ok_u64);
    c.bench_function(
        "serialize_vector_of_vector_of_u8",
        serialize_vector_of_vector_of_u8,
    );
    c.bench_function(
        "deserialize_vector_of_vector_of_u8",
        deserialize_vector_of_vector_of_u8,
    );
    c.bench_function("serialize_tree_map", serialize_tree_map);
    c.bench_function("deserialize_treemap", deserialize_treemap);
    c.bench_function("serialize_string", serialize_string);
    c.bench_function("deserialize_string", deserialize_string);
    c.bench_function("serialize_vec_of_string", serialize_vec_of_string);
    c.bench_function("deserialize_vec_of_string", deserialize_vec_of_string);
    c.bench_function("serialize_unit", serialize_unit);
    c.bench_function("deserialize_unit", deserialize_unit);
    c.bench_function("serialize_key_account", serialize_key_account);
    c.bench_function("deserialize_key_account", deserialize_key_account);
    c.bench_function("serialize_key_hash", serialize_key_hash);
    c.bench_function("deserialize_key_hash", deserialize_key_hash);
    c.bench_function("serialize_key_uref", serialize_key_uref);
    c.bench_function("deserialize_key_uref", deserialize_key_uref);
    c.bench_function("serialize_vec_of_keys", serialize_vec_of_keys);
    c.bench_function("deserialize_vec_of_keys", deserialize_vec_of_keys);
    c.bench_function("serialize_access_rights_read", serialize_access_rights_read);
    c.bench_function(
        "deserialize_access_rights_read",
        deserialize_access_rights_read,
    );
    c.bench_function(
        "serialize_access_rights_write",
        serialize_access_rights_write,
    );
    c.bench_function(
        "deserialize_access_rights_write",
        deserialize_access_rights_write,
    );
    c.bench_function("serialize_access_rights_add", serialize_access_rights_add);
    c.bench_function(
        "deserialize_access_rights_add",
        deserialize_access_rights_add,
    );
    c.bench_function(
        "serialize_access_rights_read_add",
        serialize_access_rights_read_add,
    );
    c.bench_function(
        "deserialize_access_rights_read_add",
        deserialize_access_rights_read_add,
    );
    c.bench_function(
        "serialize_access_rights_read_write",
        serialize_access_rights_read_write,
    );
    c.bench_function(
        "deserialize_access_rights_read_write",
        deserialize_access_rights_read_write,
    );
    c.bench_function(
        "serialize_access_rights_add_write",
        serialize_access_rights_add_write,
    );
    c.bench_function(
        "deserialize_access_rights_add_write",
        deserialize_access_rights_add_write,
    );
    c.bench_function("serialize_cl_value_int32", serialize_cl_value_int32);
    c.bench_function("deserialize_cl_value_int32", deserialize_cl_value_int32);
    c.bench_function("serialize_cl_value_uint128", serialize_cl_value_uint128);
    c.bench_function("deserialize_cl_value_uint128", deserialize_cl_value_uint128);
    c.bench_function("serialize_cl_value_uint256", serialize_cl_value_uint256);
    c.bench_function("deserialize_cl_value_uint256", deserialize_cl_value_uint256);
    c.bench_function("serialize_cl_value_uint512", serialize_cl_value_uint512);
    c.bench_function("deserialize_cl_value_uint512", deserialize_cl_value_uint512);
    c.bench_function("serialize_cl_value_bytearray", serialize_cl_value_bytearray);
    c.bench_function(
        "deserialize_cl_value_bytearray",
        deserialize_cl_value_bytearray,
    );
    c.bench_function("serialize_cl_value_listint32", serialize_cl_value_listint32);
    c.bench_function(
        "deserialize_cl_value_listint32",
        deserialize_cl_value_listint32,
    );
    c.bench_function("serialize_cl_value_string", serialize_cl_value_string);
    c.bench_function("deserialize_cl_value_string", deserialize_cl_value_string);
    c.bench_function(
        "serialize_cl_value_liststring",
        serialize_cl_value_liststring,
    );
    c.bench_function(
        "deserialize_cl_value_liststring",
        deserialize_cl_value_liststring,
    );
    c.bench_function("serialize_cl_value_namedkey", serialize_cl_value_namedkey);
    c.bench_function(
        "deserialize_cl_value_namedkey",
        deserialize_cl_value_namedkey,
    );
    c.bench_function("serialize_u128", serialize_u128);
    c.bench_function("deserialize_u128", deserialize_u128);
    c.bench_function("serialize_u256", serialize_u256);
    c.bench_function("deserialize_u256", deserialize_u256);
    c.bench_function("serialize_u512", serialize_u512);
    c.bench_function("deserialize_u512", deserialize_u512);
    c.bench_function("bytesrepr::serialize_account", serialize_account);
    c.bench_function("bytesrepr::deserialize_account", deserialize_account);
    c.bench_function("bytesrepr::serialize_contract", serialize_contract);
    c.bench_function("bytesrepr::deserialize_contract", deserialize_contract);
    c.bench_function(
        "bytesrepr::serialize_contract_package",
        serialize_contract_package,
    );
    c.bench_function(
        "bytesrepr::deserialize_contract_package",
        deserialize_contract_package,
    );
    c.bench_function("bytesrepr::serialize_bid_small", |b| serialize_bid(10, b));
    c.bench_function("bytesrepr::serialize_bid_medium", |b| serialize_bid(100, b));
    c.bench_function("bytesrepr::serialize_bid_big", |b| serialize_bid(1000, b));
    c.bench_function("bytesrepr::deserialize_bid_small", |b| {
        deserialize_bid(10, b)
    });
    c.bench_function("bytesrepr::deserialize_bid_medium", |b| {
        deserialize_bid(100, b)
    });
    c.bench_function("bytesrepr::deserialize_bid_big", |b| {
        deserialize_bid(1000, b)
    });
    c.bench_function("bytesrepr::serialize_transfer", serialize_transfer);
    c.bench_function("bytesrepr::deserialize_transfer", deserialize_transfer);
    c.bench_function("bytesrepr::serialize_deploy_info", serialize_deploy_info);
    c.bench_function(
        "bytesrepr::deserialize_deploy_info",
        deserialize_deploy_info,
    );
    c.bench_function("bytesrepr::serialize_era_info", |b| {
        serialize_era_info(500, b)
    });
    c.bench_function("bytesrepr::deserialize_era_info", |b| {
        deserialize_era_info(500, b)
    });
}

criterion_group!(benches, bytesrepr_bench);
criterion_main!(benches);
