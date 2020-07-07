#![feature(test)]

extern crate test;

use std::{collections::BTreeMap, iter};

use test::{black_box, Bencher};

use casperlabs_types::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes},
    AccessRights, CLTyped, CLValue, Key, URef, U128, U256, U512,
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

#[bench]
fn serialize_vector_of_i32s(b: &mut Bencher) {
    let data = prepare_vector(black_box(BATCH));
    b.iter(|| data.to_bytes());
}

#[bench]
fn deserialize_vector_of_i32s(b: &mut Bencher) {
    let data = prepare_vector(black_box(BATCH)).to_bytes().unwrap();
    b.iter(|| {
        let (res, _rem): (Vec<i32>, _) = FromBytes::from_bytes(&data).unwrap();
        res
    });
}

#[bench]
fn serialize_vector_of_u8(b: &mut Bencher) {
    // 0, 1, ... 254, 255, 0, 1, ...
    let data: Vec<u8> = prepare_vector(BATCH)
        .into_iter()
        .map(|value| value as u8)
        .collect::<Vec<_>>();
    b.iter(|| data.to_bytes());
}

#[bench]
fn deserialize_vector_of_u8(b: &mut Bencher) {
    // 0, 1, ... 254, 255, 0, 1, ...
    let data: Vec<u8> = prepare_vector(BATCH)
        .into_iter()
        .map(|value| value as u8)
        .collect::<Vec<_>>()
        .to_bytes()
        .unwrap();
    b.iter(|| Vec::<u8>::from_bytes(&data))
}

#[bench]
fn serialize_u8(b: &mut Bencher) {
    b.iter(|| ToBytes::to_bytes(black_box(&129u8)));
}

#[bench]
fn deserialize_u8(b: &mut Bencher) {
    b.iter(|| u8::from_bytes(black_box(&[129u8])));
}

#[bench]
fn serialize_i32(b: &mut Bencher) {
    b.iter(|| ToBytes::to_bytes(black_box(&1_816_142_132i32)));
}

#[bench]
fn deserialize_i32(b: &mut Bencher) {
    b.iter(|| i32::from_bytes(black_box(&[0x34, 0x21, 0x40, 0x6c])));
}

#[bench]
fn serialize_u64(b: &mut Bencher) {
    b.iter(|| ToBytes::to_bytes(black_box(&14_157_907_845_468_752_670u64)));
}

#[bench]
fn deserialize_u64(b: &mut Bencher) {
    b.iter(|| u64::from_bytes(black_box(&[0x1e, 0x8b, 0xe1, 0x73, 0x2c, 0xfe, 0x7a, 0xc4])));
}

#[bench]
fn serialize_some_u64(b: &mut Bencher) {
    let data = Some(14_157_907_845_468_752_670u64);

    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

#[bench]
fn deserialize_some_u64(b: &mut Bencher) {
    let data = Some(14_157_907_845_468_752_670u64);
    let data = data.to_bytes().unwrap();

    b.iter(|| Option::<u64>::from_bytes(&data));
}

#[bench]
fn serialize_none_u64(b: &mut Bencher) {
    let data: Option<u64> = None;

    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

#[bench]
fn deserialize_ok_u64(b: &mut Bencher) {
    let data: Option<u64> = None;
    let data = data.to_bytes().unwrap();
    b.iter(|| Option::<u64>::from_bytes(&data));
}

#[bench]
fn serialize_vector_of_vector_of_u8(b: &mut Bencher) {
    let data: Vec<Vec<u8>> = (0..4)
        .map(|_v| {
            // 0, 1, 2, ..., 254, 255
            iter::repeat_with(|| 0..255u8)
                .flatten()
                // 4 times to create 4x 1024 bytes
                .take(4)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<Vec<_>>>();

    b.iter(|| data.to_bytes());
}

#[bench]
fn deserialize_vector_of_vector_of_u8(b: &mut Bencher) {
    let data: Vec<u8> = (0..4)
        .map(|_v| {
            // 0, 1, 2, ..., 254, 255
            iter::repeat_with(|| 0..255u8)
                .flatten()
                // 4 times to create 4x 1024 bytes
                .take(4)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<Vec<_>>>()
        .to_bytes()
        .unwrap();
    b.iter(|| Vec::<Vec<u8>>::from_bytes(&data));
}

#[bench]
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

#[bench]
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

#[bench]
fn serialize_string(b: &mut Bencher) {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let data = lorem.to_string();
    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

#[bench]
fn deserialize_string(b: &mut Bencher) {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
    let data = lorem.to_bytes().unwrap();
    b.iter(|| String::from_bytes(&data));
}

#[bench]
fn serialize_vec_of_string(b: &mut Bencher) {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.".to_string();
    let array_of_lorem: Vec<String> = lorem.split(' ').map(Into::into).collect();
    let data = array_of_lorem;
    b.iter(|| ToBytes::to_bytes(black_box(&data)));
}

#[bench]
fn deserialize_vec_of_string(b: &mut Bencher) {
    let lorem = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.".to_string();
    let array_of_lorem: Vec<String> = lorem.split(' ').map(Into::into).collect();
    let data = array_of_lorem.to_bytes().unwrap();

    b.iter(|| Vec::<String>::from_bytes(&data));
}

#[bench]
fn serialize_unit(b: &mut Bencher) {
    b.iter(|| ToBytes::to_bytes(black_box(&())))
}

#[bench]
fn deserialize_unit(b: &mut Bencher) {
    let data = ().to_bytes().unwrap();

    b.iter(|| <()>::from_bytes(&data))
}

#[bench]
fn serialize_key_account(b: &mut Bencher) {
    let account = Key::Account(AccountHash::new([0u8; 32]));

    b.iter(|| ToBytes::to_bytes(black_box(&account)))
}

#[bench]
fn deserialize_key_account(b: &mut Bencher) {
    let account = Key::Account(AccountHash::new([0u8; 32]));
    let account_bytes = account.to_bytes().unwrap();

    b.iter(|| Key::from_bytes(black_box(&account_bytes)))
}

#[bench]
fn serialize_key_hash(b: &mut Bencher) {
    let hash = Key::Hash([0u8; 32]);
    b.iter(|| ToBytes::to_bytes(black_box(&hash)))
}

#[bench]
fn deserialize_key_hash(b: &mut Bencher) {
    let hash = Key::Hash([0u8; 32]);
    let hash_bytes = hash.to_bytes().unwrap();

    b.iter(|| Key::from_bytes(black_box(&hash_bytes)))
}

#[bench]
fn serialize_key_uref(b: &mut Bencher) {
    let uref = Key::URef(URef::new([0u8; 32], AccessRights::ADD_WRITE));
    b.iter(|| ToBytes::to_bytes(black_box(&uref)))
}

#[bench]
fn deserialize_key_uref(b: &mut Bencher) {
    let uref = Key::URef(URef::new([0u8; 32], AccessRights::ADD_WRITE));
    let uref_bytes = uref.to_bytes().unwrap();

    b.iter(|| Key::from_bytes(black_box(&uref_bytes)))
}

#[bench]
fn serialize_vec_of_keys(b: &mut Bencher) {
    let keys: Vec<Key> = (0..32)
        .map(|i| Key::URef(URef::new([i; 32], AccessRights::ADD_WRITE)))
        .collect();
    b.iter(|| ToBytes::to_bytes(black_box(&keys)))
}

#[bench]
fn deserialize_vec_of_keys(b: &mut Bencher) {
    let keys: Vec<Key> = (0..32)
        .map(|i| Key::URef(URef::new([i; 32], AccessRights::ADD_WRITE)))
        .collect();
    let keys_bytes = keys.to_bytes().unwrap();
    b.iter(|| Vec::<Key>::from_bytes(black_box(&keys_bytes)));
}

#[bench]
fn serialize_access_rights_read(b: &mut Bencher) {
    b.iter(|| AccessRights::READ.to_bytes());
}

#[bench]
fn deserialize_access_rights_read(b: &mut Bencher) {
    let data = AccessRights::READ.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

#[bench]
fn serialize_access_rights_write(b: &mut Bencher) {
    b.iter(|| AccessRights::WRITE.to_bytes());
}

#[bench]
fn deserialize_access_rights_write(b: &mut Bencher) {
    let data = AccessRights::WRITE.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

#[bench]
fn serialize_access_rights_add(b: &mut Bencher) {
    b.iter(|| AccessRights::ADD.to_bytes());
}

#[bench]
fn deserialize_access_rights_add(b: &mut Bencher) {
    let data = AccessRights::ADD.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

#[bench]
fn serialize_access_rights_read_add(b: &mut Bencher) {
    b.iter(|| AccessRights::READ_ADD.to_bytes());
}

#[bench]
fn deserialize_access_rights_read_add(b: &mut Bencher) {
    let data = AccessRights::READ_ADD.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

#[bench]
fn serialize_access_rights_read_write(b: &mut Bencher) {
    b.iter(|| AccessRights::READ_WRITE.to_bytes());
}

#[bench]
fn deserialize_access_rights_read_write(b: &mut Bencher) {
    let data = AccessRights::READ_WRITE.to_bytes().unwrap();
    b.iter(|| AccessRights::from_bytes(&data));
}

#[bench]
fn serialize_access_rights_add_write(b: &mut Bencher) {
    b.iter(|| AccessRights::ADD_WRITE.to_bytes());
}

#[bench]
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
        let cl_value: CLValue = bytesrepr::deserialize(serialized_value.clone()).unwrap();
        let _raw_value: T = cl_value.into_t().unwrap();
    });
}

#[bench]
fn serialize_cl_value_int32(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_I32));
}

#[bench]
fn deserialize_cl_value_int32(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_I32);
}

#[bench]
fn serialize_cl_value_uint128(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_U128));
}

#[bench]
fn deserialize_cl_value_uint128(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_U128);
}

#[bench]
fn serialize_cl_value_uint256(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_U256));
}

#[bench]
fn deserialize_cl_value_uint256(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_U256);
}

#[bench]
fn serialize_cl_value_uint512(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_U512));
}

#[bench]
fn deserialize_cl_value_uint512(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_U512);
}

#[bench]
fn serialize_cl_value_bytearray(b: &mut Bencher) {
    b.iter(|| serialize_cl_value((0..255).collect::<Vec<u8>>()));
}

#[bench]
fn deserialize_cl_value_bytearray(b: &mut Bencher) {
    benchmark_deserialization(b, (0..255).collect::<Vec<u8>>());
}

#[bench]
fn serialize_cl_value_listint32(b: &mut Bencher) {
    b.iter(|| serialize_cl_value((0..1024).collect::<Vec<i32>>()));
}

#[bench]
fn deserialize_cl_value_listint32(b: &mut Bencher) {
    benchmark_deserialization(b, (0..1024).collect::<Vec<i32>>());
}

#[bench]
fn serialize_cl_value_string(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(TEST_STR_1.to_string()));
}

#[bench]
fn deserialize_cl_value_string(b: &mut Bencher) {
    benchmark_deserialization(b, TEST_STR_1.to_string());
}

#[bench]
fn serialize_cl_value_liststring(b: &mut Bencher) {
    b.iter(|| serialize_cl_value(vec![TEST_STR_1.to_string(), TEST_STR_2.to_string()]));
}

#[bench]
fn deserialize_cl_value_liststring(b: &mut Bencher) {
    benchmark_deserialization(b, vec![TEST_STR_1.to_string(), TEST_STR_2.to_string()]);
}

#[bench]
fn serialize_cl_value_namedkey(b: &mut Bencher) {
    b.iter(|| {
        serialize_cl_value((
            TEST_STR_1.to_string(),
            Key::Account(AccountHash::new([0xffu8; 32])),
        ))
    });
}

#[bench]
fn deserialize_cl_value_namedkey(b: &mut Bencher) {
    benchmark_deserialization(
        b,
        (
            TEST_STR_1.to_string(),
            Key::Account(AccountHash::new([0xffu8; 32])),
        ),
    );
}

#[bench]
fn serialize_u128(b: &mut Bencher) {
    let num_u128 = U128::default();
    b.iter(|| ToBytes::to_bytes(black_box(&num_u128)))
}

#[bench]
fn deserialize_u128(b: &mut Bencher) {
    let num_u128 = U128::default();
    let num_u128_bytes = num_u128.to_bytes().unwrap();

    b.iter(|| U128::from_bytes(black_box(&num_u128_bytes)))
}

#[bench]
fn serialize_u256(b: &mut Bencher) {
    let num_u256 = U256::default();
    b.iter(|| ToBytes::to_bytes(black_box(&num_u256)))
}

#[bench]
fn deserialize_u256(b: &mut Bencher) {
    let num_u256 = U256::default();
    let num_u256_bytes = num_u256.to_bytes().unwrap();

    b.iter(|| U256::from_bytes(black_box(&num_u256_bytes)))
}

#[bench]
fn serialize_u512(b: &mut Bencher) {
    let num_u512 = U512::default();
    b.iter(|| ToBytes::to_bytes(black_box(&num_u512)))
}

#[bench]
fn deserialize_u512(b: &mut Bencher) {
    let num_u512 = U512::default();
    let num_u512_bytes = num_u512.to_bytes().unwrap();

    b.iter(|| U512::from_bytes(black_box(&num_u512_bytes)))
}
