use alloc::{string::ToString, vec::Vec};
use core::{any, convert::TryFrom};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use num::traits::{AsPrimitive, WrappingAdd};
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::error;

use super::TransformError;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contracts::NamedKeys,
    CLType, CLTyped, CLValue, Key, StoredValue, StoredValueTypeMismatch, U128, U256, U512,
};

/// Taxonomy of Transform.
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TransformInstruction {
    /// Store a StoredValue.
    Store(StoredValue),
    /// Prune a StoredValue by Key.
    Prune(Key),
}

impl TransformInstruction {
    /// Store instruction.
    pub fn store(stored_value: StoredValue) -> Self {
        Self::Store(stored_value)
    }

    /// Prune instruction.
    pub fn prune(key: Key) -> Self {
        Self::Prune(key)
    }
}

impl From<StoredValue> for TransformInstruction {
    fn from(value: StoredValue) -> Self {
        TransformInstruction::Store(value)
    }
}

/// Representation of a single transformation occurring during execution.
///
/// Note that all arithmetic variants of `TransformKindV2` are commutative which means that a given
/// collection of them can be executed in any order to produce the same end result.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, Default)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum TransformKindV2 {
    /// An identity transformation that does not modify a value in the global state.
    ///
    /// Created as a result of reading from the global state.
    #[default]
    Identity,
    /// Writes a new value in the global state.
    Write(StoredValue),
    /// A wrapping addition of an `i32` to an existing numeric value (not necessarily an `i32`) in
    /// the global state.
    AddInt32(i32),
    /// A wrapping addition of a `u64` to an existing numeric value (not necessarily an `u64`) in
    /// the global state.
    AddUInt64(u64),
    /// A wrapping addition of a `U128` to an existing numeric value (not necessarily an `U128`) in
    /// the global state.
    AddUInt128(U128),
    /// A wrapping addition of a `U256` to an existing numeric value (not necessarily an `U256`) in
    /// the global state.
    AddUInt256(U256),
    /// A wrapping addition of a `U512` to an existing numeric value (not necessarily an `U512`) in
    /// the global state.
    AddUInt512(U512),
    /// Adds new named keys to an existing entry in the global state.
    ///
    /// This transform assumes that the existing stored value is either an Account or a Contract.
    AddKeys(NamedKeys),
    /// Removes the pathing to the global state entry of the specified key. The pruned element
    /// remains reachable from previously generated global state root hashes, but will not be
    /// included in the next generated global state root hash and subsequent state accumulated
    /// from it.
    Prune(Key),
    /// Represents the case where applying a transform would cause an error.
    Failure(TransformError),
}

impl TransformKindV2 {
    /// Applies the transformation on a specified stored value instance.
    ///
    /// This method produces a new `StoredValue` instance based on the `TransformKind` variant.
    pub fn apply(self, stored_value: StoredValue) -> Result<TransformInstruction, TransformError> {
        fn store(sv: StoredValue) -> TransformInstruction {
            TransformInstruction::Store(sv)
        }
        match self {
            TransformKindV2::Identity => Ok(store(stored_value)),
            TransformKindV2::Write(new_value) => Ok(store(new_value)),
            TransformKindV2::Prune(key) => Ok(TransformInstruction::prune(key)),
            TransformKindV2::AddInt32(to_add) => wrapping_addition(stored_value, to_add),
            TransformKindV2::AddUInt64(to_add) => wrapping_addition(stored_value, to_add),
            TransformKindV2::AddUInt128(to_add) => wrapping_addition(stored_value, to_add),
            TransformKindV2::AddUInt256(to_add) => wrapping_addition(stored_value, to_add),
            TransformKindV2::AddUInt512(to_add) => wrapping_addition(stored_value, to_add),
            TransformKindV2::AddKeys(keys) => match stored_value {
                StoredValue::Contract(mut contract) => {
                    contract.named_keys_append(keys);
                    Ok(store(StoredValue::Contract(contract)))
                }
                StoredValue::Account(mut account) => {
                    account.named_keys_append(keys);
                    Ok(store(StoredValue::Account(account)))
                }
                StoredValue::AddressableEntity(_) => Err(TransformError::Deprecated),
                StoredValue::CLValue(cl_value) => {
                    let expected = "Contract or Account".to_string();
                    let found = format!("{:?}", cl_value.cl_type());
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::SmartContract(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "ContractPackage".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::ByteCode(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "ByteCode".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::Transfer(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "Transfer".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::DeployInfo(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "DeployInfo".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::EraInfo(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "EraInfo".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::Bid(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "Bid".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::BidKind(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "BidKind".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::Withdraw(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "Withdraw".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::Unbonding(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "Unbonding".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::ContractWasm(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "ContractWasm".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::ContractPackage(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "ContractPackage".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::NamedKey(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "NamedKeyValue".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::MessageTopic(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "MessageTopic".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::Message(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "Message".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::RawBytes(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "RawBytes".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::Prepayment(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "Prepayment".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
                StoredValue::EntryPoint(_) => {
                    let expected = "Contract or Account".to_string();
                    let found = "EntryPoint".to_string();
                    Err(StoredValueTypeMismatch::new(expected, found).into())
                }
            },
            TransformKindV2::Failure(error) => Err(error),
        }
    }

    /// Returns a random `TransformKind`.
    #[cfg(any(feature = "testing", test))]
    pub fn random<R: Rng + ?Sized>(rng: &mut R) -> Self {
        match rng.gen_range(0..10) {
            0 => TransformKindV2::Identity,
            1 => TransformKindV2::Write(StoredValue::CLValue(CLValue::from_t(true).unwrap())),
            2 => TransformKindV2::AddInt32(rng.gen()),
            3 => TransformKindV2::AddUInt64(rng.gen()),
            4 => TransformKindV2::AddUInt128(rng.gen::<u64>().into()),
            5 => TransformKindV2::AddUInt256(rng.gen::<u64>().into()),
            6 => TransformKindV2::AddUInt512(rng.gen::<u64>().into()),
            7 => {
                let mut named_keys = NamedKeys::new();
                for _ in 0..rng.gen_range(1..6) {
                    named_keys.insert(rng.gen::<u64>().to_string(), rng.gen());
                }
                TransformKindV2::AddKeys(named_keys)
            }
            8 => TransformKindV2::Failure(TransformError::Serialization(
                bytesrepr::Error::EarlyEndOfStream,
            )),
            9 => TransformKindV2::Prune(rng.gen::<Key>()),
            _ => unreachable!(),
        }
    }
}

impl ToBytes for TransformKindV2 {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TransformKindV2::Identity => 0,
                TransformKindV2::Write(stored_value) => stored_value.serialized_length(),
                TransformKindV2::AddInt32(value) => value.serialized_length(),
                TransformKindV2::AddUInt64(value) => value.serialized_length(),
                TransformKindV2::AddUInt128(value) => value.serialized_length(),
                TransformKindV2::AddUInt256(value) => value.serialized_length(),
                TransformKindV2::AddUInt512(value) => value.serialized_length(),
                TransformKindV2::AddKeys(named_keys) => named_keys.serialized_length(),
                TransformKindV2::Failure(error) => error.serialized_length(),
                TransformKindV2::Prune(value) => value.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransformKindV2::Identity => (TransformTag::Identity as u8).write_bytes(writer),
            TransformKindV2::Write(stored_value) => {
                (TransformTag::Write as u8).write_bytes(writer)?;
                stored_value.write_bytes(writer)
            }
            TransformKindV2::AddInt32(value) => {
                (TransformTag::AddInt32 as u8).write_bytes(writer)?;
                value.write_bytes(writer)
            }
            TransformKindV2::AddUInt64(value) => {
                (TransformTag::AddUInt64 as u8).write_bytes(writer)?;
                value.write_bytes(writer)
            }
            TransformKindV2::AddUInt128(value) => {
                (TransformTag::AddUInt128 as u8).write_bytes(writer)?;
                value.write_bytes(writer)
            }
            TransformKindV2::AddUInt256(value) => {
                (TransformTag::AddUInt256 as u8).write_bytes(writer)?;
                value.write_bytes(writer)
            }
            TransformKindV2::AddUInt512(value) => {
                (TransformTag::AddUInt512 as u8).write_bytes(writer)?;
                value.write_bytes(writer)
            }
            TransformKindV2::AddKeys(named_keys) => {
                (TransformTag::AddKeys as u8).write_bytes(writer)?;
                named_keys.write_bytes(writer)
            }
            TransformKindV2::Failure(error) => {
                (TransformTag::Failure as u8).write_bytes(writer)?;
                error.write_bytes(writer)
            }
            TransformKindV2::Prune(value) => {
                (TransformTag::Prune as u8).write_bytes(writer)?;
                value.write_bytes(writer)
            }
        }
    }
}

impl FromBytes for TransformKindV2 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        if bytes.is_empty() {
            error!("FromBytes for TransformKindV2: bytes length should not be 0");
        }
        let (tag, remainder) = match u8::from_bytes(bytes) {
            Ok((tag, rem)) => (tag, rem),
            Err(err) => {
                error!(%err, "FromBytes for TransformKindV2");
                return Err(err);
            }
        };
        match tag {
            tag if tag == TransformTag::Identity as u8 => {
                Ok((TransformKindV2::Identity, remainder))
            }
            tag if tag == TransformTag::Write as u8 => {
                let (stored_value, remainder) = StoredValue::from_bytes(remainder)?;
                Ok((TransformKindV2::Write(stored_value), remainder))
            }
            tag if tag == TransformTag::AddInt32 as u8 => {
                let (value, remainder) = i32::from_bytes(remainder)?;
                Ok((TransformKindV2::AddInt32(value), remainder))
            }
            tag if tag == TransformTag::AddUInt64 as u8 => {
                let (value, remainder) = u64::from_bytes(remainder)?;
                Ok((TransformKindV2::AddUInt64(value), remainder))
            }
            tag if tag == TransformTag::AddUInt128 as u8 => {
                let (value, remainder) = U128::from_bytes(remainder)?;
                Ok((TransformKindV2::AddUInt128(value), remainder))
            }
            tag if tag == TransformTag::AddUInt256 as u8 => {
                let (value, remainder) = U256::from_bytes(remainder)?;
                Ok((TransformKindV2::AddUInt256(value), remainder))
            }
            tag if tag == TransformTag::AddUInt512 as u8 => {
                let (value, remainder) = U512::from_bytes(remainder)?;
                Ok((TransformKindV2::AddUInt512(value), remainder))
            }
            tag if tag == TransformTag::AddKeys as u8 => {
                let (named_keys, remainder) = NamedKeys::from_bytes(remainder)?;
                Ok((TransformKindV2::AddKeys(named_keys), remainder))
            }
            tag if tag == TransformTag::Failure as u8 => {
                let (error, remainder) = TransformError::from_bytes(remainder)?;
                Ok((TransformKindV2::Failure(error), remainder))
            }
            tag if tag == TransformTag::Prune as u8 => {
                let (key, remainder) = Key::from_bytes(remainder)?;
                Ok((TransformKindV2::Prune(key), remainder))
            }
            _ => {
                error!(%tag, rem_len = remainder.len(), "FromBytes for TransformKindV2: unknown tag");
                Err(bytesrepr::Error::Formatting)
            }
        }
    }
}

/// Attempts a wrapping addition of `to_add` to `stored_value`, assuming `stored_value` is
/// compatible with type `Y`.
fn wrapping_addition<Y>(
    stored_value: StoredValue,
    to_add: Y,
) -> Result<TransformInstruction, TransformError>
where
    Y: AsPrimitive<i32>
        + AsPrimitive<i64>
        + AsPrimitive<u8>
        + AsPrimitive<u32>
        + AsPrimitive<u64>
        + AsPrimitive<U128>
        + AsPrimitive<U256>
        + AsPrimitive<U512>,
{
    let cl_value = CLValue::try_from(stored_value)?;

    match cl_value.cl_type() {
        CLType::I32 => do_wrapping_addition::<i32, _>(cl_value, to_add),
        CLType::I64 => do_wrapping_addition::<i64, _>(cl_value, to_add),
        CLType::U8 => do_wrapping_addition::<u8, _>(cl_value, to_add),
        CLType::U32 => do_wrapping_addition::<u32, _>(cl_value, to_add),
        CLType::U64 => do_wrapping_addition::<u64, _>(cl_value, to_add),
        CLType::U128 => do_wrapping_addition::<U128, _>(cl_value, to_add),
        CLType::U256 => do_wrapping_addition::<U256, _>(cl_value, to_add),
        CLType::U512 => do_wrapping_addition::<U512, _>(cl_value, to_add),
        other => {
            let expected = format!("integral type compatible with {}", any::type_name::<Y>());
            let found = format!("{:?}", other);
            Err(StoredValueTypeMismatch::new(expected, found).into())
        }
    }
}

/// Attempts a wrapping addition of `to_add` to the value represented by `cl_value`.
fn do_wrapping_addition<X, Y>(
    cl_value: CLValue,
    to_add: Y,
) -> Result<TransformInstruction, TransformError>
where
    X: WrappingAdd + CLTyped + ToBytes + FromBytes + Copy + 'static,
    Y: AsPrimitive<X>,
{
    let x: X = cl_value.into_t()?;
    let result = x.wrapping_add(&(to_add.as_()));
    let stored_value = StoredValue::CLValue(CLValue::from_t(result)?);
    Ok(TransformInstruction::store(stored_value))
}

#[derive(Debug)]
#[repr(u8)]
enum TransformTag {
    Identity = 0,
    Write = 1,
    AddInt32 = 2,
    AddUInt64 = 3,
    AddUInt128 = 4,
    AddUInt256 = 5,
    AddUInt512 = 6,
    AddKeys = 7,
    Failure = 8,
    Prune = 9,
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fmt};

    use num::{Bounded, Num};

    use crate::{
        byte_code::ByteCodeKind, bytesrepr::Bytes, testing::TestRng, AccessRights, ByteCode, Key,
        URef, U128, U256, U512,
    };

    use super::*;

    const ZERO_ARRAY: [u8; 32] = [0; 32];
    const TEST_STR: &str = "a";
    const TEST_BOOL: bool = true;

    const ZERO_I32: i32 = 0;
    const ONE_I32: i32 = 1;
    const NEG_ONE_I32: i32 = -1;
    const NEG_TWO_I32: i32 = -2;
    const MIN_I32: i32 = i32::MIN;
    const MAX_I32: i32 = i32::MAX;

    const ZERO_I64: i64 = 0;
    const ONE_I64: i64 = 1;
    const NEG_ONE_I64: i64 = -1;
    const NEG_TWO_I64: i64 = -2;
    const MIN_I64: i64 = i64::MIN;
    const MAX_I64: i64 = i64::MAX;

    const ZERO_U8: u8 = 0;
    const ONE_U8: u8 = 1;
    const MAX_U8: u8 = u8::MAX;

    const ZERO_U32: u32 = 0;
    const ONE_U32: u32 = 1;
    const MAX_U32: u32 = u32::MAX;

    const ZERO_U64: u64 = 0;
    const ONE_U64: u64 = 1;
    const MAX_U64: u64 = u64::MAX;

    const ZERO_U128: U128 = U128([0; 2]);
    const ONE_U128: U128 = U128([1, 0]);
    const MAX_U128: U128 = U128([MAX_U64; 2]);

    const ZERO_U256: U256 = U256([0; 4]);
    const ONE_U256: U256 = U256([1, 0, 0, 0]);
    const MAX_U256: U256 = U256([MAX_U64; 4]);

    const ZERO_U512: U512 = U512([0; 8]);
    const ONE_U512: U512 = U512([1, 0, 0, 0, 0, 0, 0, 0]);
    const MAX_U512: U512 = U512([MAX_U64; 8]);

    impl From<U128> for TransformKindV2 {
        fn from(x: U128) -> Self {
            TransformKindV2::AddUInt128(x)
        }
    }
    impl From<U256> for TransformKindV2 {
        fn from(x: U256) -> Self {
            TransformKindV2::AddUInt256(x)
        }
    }
    impl From<U512> for TransformKindV2 {
        fn from(x: U512) -> Self {
            TransformKindV2::AddUInt512(x)
        }
    }

    #[test]
    fn i32_overflow() {
        let max = i32::MAX;
        let min = i32::MIN;

        let max_value = StoredValue::CLValue(CLValue::from_t(max).unwrap());
        let min_value = StoredValue::CLValue(CLValue::from_t(min).unwrap());

        let apply_overflow = TransformKindV2::AddInt32(1).apply(max_value.clone());
        let apply_underflow = TransformKindV2::AddInt32(-1).apply(min_value.clone());

        assert_eq!(
            apply_overflow.expect("Unexpected overflow"),
            TransformInstruction::store(min_value)
        );
        assert_eq!(
            apply_underflow.expect("Unexpected underflow"),
            TransformInstruction::store(max_value)
        );
    }

    fn uint_overflow_test<T>()
    where
        T: Num + Bounded + CLTyped + ToBytes + Into<TransformKindV2> + Copy,
    {
        let max = T::max_value();
        let min = T::min_value();
        let one = T::one();
        let zero = T::zero();

        let max_value = StoredValue::CLValue(CLValue::from_t(max).unwrap());
        let min_value = StoredValue::CLValue(CLValue::from_t(min).unwrap());
        let zero_value = StoredValue::CLValue(CLValue::from_t(zero).unwrap());

        let one_transform: TransformKindV2 = one.into();

        let apply_overflow = TransformKindV2::AddInt32(1).apply(max_value.clone());

        let apply_overflow_uint = one_transform.apply(max_value.clone());
        let apply_underflow = TransformKindV2::AddInt32(-1).apply(min_value);

        assert_eq!(apply_overflow, Ok(zero_value.clone().into()));
        assert_eq!(apply_overflow_uint, Ok(zero_value.into()));
        assert_eq!(apply_underflow, Ok(max_value.into()));
    }

    #[test]
    fn u128_overflow() {
        uint_overflow_test::<U128>();
    }

    #[test]
    fn u256_overflow() {
        uint_overflow_test::<U256>();
    }

    #[test]
    fn u512_overflow() {
        uint_overflow_test::<U512>();
    }

    #[test]
    fn addition_between_mismatched_types_should_fail() {
        fn assert_yields_type_mismatch_error(stored_value: StoredValue) {
            match wrapping_addition(stored_value, ZERO_I32) {
                Err(TransformError::TypeMismatch(_)) => (),
                _ => panic!("wrapping addition should yield TypeMismatch error"),
            };
        }

        let byte_code = StoredValue::ByteCode(ByteCode::new(ByteCodeKind::V1CasperWasm, vec![]));
        assert_yields_type_mismatch_error(byte_code);

        let uref = URef::new(ZERO_ARRAY, AccessRights::READ);

        let cl_bool =
            StoredValue::CLValue(CLValue::from_t(TEST_BOOL).expect("should create CLValue"));
        assert_yields_type_mismatch_error(cl_bool);

        let cl_unit = StoredValue::CLValue(CLValue::from_t(()).expect("should create CLValue"));
        assert_yields_type_mismatch_error(cl_unit);

        let cl_string =
            StoredValue::CLValue(CLValue::from_t(TEST_STR).expect("should create CLValue"));
        assert_yields_type_mismatch_error(cl_string);

        let cl_key = StoredValue::CLValue(
            CLValue::from_t(Key::Hash(ZERO_ARRAY)).expect("should create CLValue"),
        );
        assert_yields_type_mismatch_error(cl_key);

        let cl_uref = StoredValue::CLValue(CLValue::from_t(uref).expect("should create CLValue"));
        assert_yields_type_mismatch_error(cl_uref);

        let cl_option =
            StoredValue::CLValue(CLValue::from_t(Some(ZERO_U8)).expect("should create CLValue"));
        assert_yields_type_mismatch_error(cl_option);

        let cl_list = StoredValue::CLValue(
            CLValue::from_t(Bytes::from(vec![ZERO_U8])).expect("should create CLValue"),
        );
        assert_yields_type_mismatch_error(cl_list);

        let cl_fixed_list =
            StoredValue::CLValue(CLValue::from_t([ZERO_U8]).expect("should create CLValue"));
        assert_yields_type_mismatch_error(cl_fixed_list);

        let cl_result: Result<(), u8> = Err(ZERO_U8);
        let cl_result =
            StoredValue::CLValue(CLValue::from_t(cl_result).expect("should create CLValue"));
        assert_yields_type_mismatch_error(cl_result);

        let cl_map = StoredValue::CLValue(
            CLValue::from_t(BTreeMap::<u8, u8>::new()).expect("should create CLValue"),
        );
        assert_yields_type_mismatch_error(cl_map);

        let cl_tuple1 =
            StoredValue::CLValue(CLValue::from_t((ZERO_U8,)).expect("should create CLValue"));
        assert_yields_type_mismatch_error(cl_tuple1);

        let cl_tuple2 = StoredValue::CLValue(
            CLValue::from_t((ZERO_U8, ZERO_U8)).expect("should create CLValue"),
        );
        assert_yields_type_mismatch_error(cl_tuple2);

        let cl_tuple3 = StoredValue::CLValue(
            CLValue::from_t((ZERO_U8, ZERO_U8, ZERO_U8)).expect("should create CLValue"),
        );
        assert_yields_type_mismatch_error(cl_tuple3);
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn wrapping_addition_should_succeed() {
        fn add<X, Y>(current_value: X, to_add: Y) -> X
        where
            X: CLTyped + ToBytes + FromBytes + PartialEq + fmt::Debug,
            Y: AsPrimitive<i32>
                + AsPrimitive<i64>
                + AsPrimitive<u8>
                + AsPrimitive<u32>
                + AsPrimitive<u64>
                + AsPrimitive<U128>
                + AsPrimitive<U256>
                + AsPrimitive<U512>,
        {
            let current = StoredValue::CLValue(
                CLValue::from_t(current_value).expect("should create CLValue"),
            );
            if let TransformInstruction::Store(result) =
                wrapping_addition(current, to_add).expect("wrapping addition should succeed")
            {
                CLValue::try_from(result)
                    .expect("should be CLValue")
                    .into_t()
                    .expect("should parse to X")
            } else {
                panic!("expected TransformInstruction::Store");
            }
        }

        // Adding to i32
        assert_eq!(ONE_I32, add(ZERO_I32, ONE_I32));
        assert_eq!(MIN_I32, add(MAX_I32, ONE_I32));
        assert_eq!(NEG_TWO_I32, add(MAX_I32, MAX_I32));
        assert_eq!(ZERO_I32, add(ONE_I32, NEG_ONE_I32));
        assert_eq!(NEG_ONE_I32, add(ZERO_I32, NEG_ONE_I32));
        assert_eq!(MAX_I32, add(NEG_ONE_I32, MIN_I32));

        assert_eq!(ONE_I32, add(ZERO_I32, ONE_U64));
        assert_eq!(MIN_I32, add(MAX_I32, ONE_U64));
        assert_eq!(NEG_TWO_I32, add(MAX_I32, MAX_I32 as u64));

        assert_eq!(ONE_I32, add(ZERO_I32, ONE_U128));
        assert_eq!(MIN_I32, add(MAX_I32, ONE_U128));
        assert_eq!(NEG_TWO_I32, add(MAX_I32, U128::from(MAX_I32)));

        assert_eq!(ONE_I32, add(ZERO_I32, ONE_U256));
        assert_eq!(MIN_I32, add(MAX_I32, ONE_U256));
        assert_eq!(NEG_TWO_I32, add(MAX_I32, U256::from(MAX_I32)));

        assert_eq!(ONE_I32, add(ZERO_I32, ONE_U512));
        assert_eq!(MIN_I32, add(MAX_I32, ONE_U512));
        assert_eq!(NEG_TWO_I32, add(MAX_I32, U512::from(MAX_I32)));

        // Adding to i64
        assert_eq!(ONE_I64, add(ZERO_I64, ONE_I32));
        assert_eq!(MIN_I64, add(MAX_I64, ONE_I32));
        assert_eq!(ZERO_I64, add(ONE_I64, NEG_ONE_I32));
        assert_eq!(NEG_ONE_I64, add(ZERO_I64, NEG_ONE_I32));
        assert_eq!(MAX_I64, add(MIN_I64, NEG_ONE_I32));

        assert_eq!(ONE_I64, add(ZERO_I64, ONE_U64));
        assert_eq!(MIN_I64, add(MAX_I64, ONE_U64));
        assert_eq!(NEG_TWO_I64, add(MAX_I64, MAX_I64 as u64));

        assert_eq!(ONE_I64, add(ZERO_I64, ONE_U128));
        assert_eq!(MIN_I64, add(MAX_I64, ONE_U128));
        assert_eq!(NEG_TWO_I64, add(MAX_I64, U128::from(MAX_I64)));

        assert_eq!(ONE_I64, add(ZERO_I64, ONE_U256));
        assert_eq!(MIN_I64, add(MAX_I64, ONE_U256));
        assert_eq!(NEG_TWO_I64, add(MAX_I64, U256::from(MAX_I64)));

        assert_eq!(ONE_I64, add(ZERO_I64, ONE_U512));
        assert_eq!(MIN_I64, add(MAX_I64, ONE_U512));
        assert_eq!(NEG_TWO_I64, add(MAX_I64, U512::from(MAX_I64)));

        // Adding to u8
        assert_eq!(ONE_U8, add(ZERO_U8, ONE_I32));
        assert_eq!(ZERO_U8, add(MAX_U8, ONE_I32));
        assert_eq!(MAX_U8, add(MAX_U8, 256_i32));
        assert_eq!(ZERO_U8, add(MAX_U8, 257_i32));
        assert_eq!(ZERO_U8, add(ONE_U8, NEG_ONE_I32));
        assert_eq!(MAX_U8, add(ZERO_U8, NEG_ONE_I32));
        assert_eq!(ZERO_U8, add(ZERO_U8, -256_i32));
        assert_eq!(MAX_U8, add(ZERO_U8, -257_i32));
        assert_eq!(MAX_U8, add(ZERO_U8, MAX_I32));
        assert_eq!(ZERO_U8, add(ZERO_U8, MIN_I32));

        assert_eq!(ONE_U8, add(ZERO_U8, ONE_U64));
        assert_eq!(ZERO_U8, add(MAX_U8, ONE_U64));
        assert_eq!(ONE_U8, add(ZERO_U8, u64::from(MAX_U8) + 2));
        assert_eq!(MAX_U8, add(ZERO_U8, MAX_U64));

        assert_eq!(ONE_U8, add(ZERO_U8, ONE_U128));
        assert_eq!(ZERO_U8, add(MAX_U8, ONE_U128));
        assert_eq!(ONE_U8, add(ZERO_U8, U128::from(MAX_U8) + 2));
        assert_eq!(MAX_U8, add(ZERO_U8, MAX_U128));

        assert_eq!(ONE_U8, add(ZERO_U8, ONE_U256));
        assert_eq!(ZERO_U8, add(MAX_U8, ONE_U256));
        assert_eq!(ONE_U8, add(ZERO_U8, U256::from(MAX_U8) + 2));
        assert_eq!(MAX_U8, add(ZERO_U8, MAX_U256));

        assert_eq!(ONE_U8, add(ZERO_U8, ONE_U512));
        assert_eq!(ZERO_U8, add(MAX_U8, ONE_U512));
        assert_eq!(ONE_U8, add(ZERO_U8, U512::from(MAX_U8) + 2));
        assert_eq!(MAX_U8, add(ZERO_U8, MAX_U512));

        // Adding to u32
        assert_eq!(ONE_U32, add(ZERO_U32, ONE_I32));
        assert_eq!(ZERO_U32, add(MAX_U32, ONE_I32));
        assert_eq!(ZERO_U32, add(ONE_U32, NEG_ONE_I32));
        assert_eq!(MAX_U32, add(ZERO_U32, NEG_ONE_I32));
        assert_eq!(MAX_I32 as u32 + 1, add(ZERO_U32, MIN_I32));

        assert_eq!(ONE_U32, add(ZERO_U32, ONE_U64));
        assert_eq!(ZERO_U32, add(MAX_U32, ONE_U64));
        assert_eq!(ONE_U32, add(ZERO_U32, u64::from(MAX_U32) + 2));
        assert_eq!(MAX_U32, add(ZERO_U32, MAX_U64));

        assert_eq!(ONE_U32, add(ZERO_U32, ONE_U128));
        assert_eq!(ZERO_U32, add(MAX_U32, ONE_U128));
        assert_eq!(ONE_U32, add(ZERO_U32, U128::from(MAX_U32) + 2));
        assert_eq!(MAX_U32, add(ZERO_U32, MAX_U128));

        assert_eq!(ONE_U32, add(ZERO_U32, ONE_U256));
        assert_eq!(ZERO_U32, add(MAX_U32, ONE_U256));
        assert_eq!(ONE_U32, add(ZERO_U32, U256::from(MAX_U32) + 2));
        assert_eq!(MAX_U32, add(ZERO_U32, MAX_U256));

        assert_eq!(ONE_U32, add(ZERO_U32, ONE_U512));
        assert_eq!(ZERO_U32, add(MAX_U32, ONE_U512));
        assert_eq!(ONE_U32, add(ZERO_U32, U512::from(MAX_U32) + 2));
        assert_eq!(MAX_U32, add(ZERO_U32, MAX_U512));

        // Adding to u64
        assert_eq!(ONE_U64, add(ZERO_U64, ONE_I32));
        assert_eq!(ZERO_U64, add(MAX_U64, ONE_I32));
        assert_eq!(ZERO_U64, add(ONE_U64, NEG_ONE_I32));
        assert_eq!(MAX_U64, add(ZERO_U64, NEG_ONE_I32));

        assert_eq!(ONE_U64, add(ZERO_U64, ONE_U64));
        assert_eq!(ZERO_U64, add(MAX_U64, ONE_U64));
        assert_eq!(MAX_U64 - 1, add(MAX_U64, MAX_U64));

        assert_eq!(ONE_U64, add(ZERO_U64, ONE_U128));
        assert_eq!(ZERO_U64, add(MAX_U64, ONE_U128));
        assert_eq!(ONE_U64, add(ZERO_U64, U128::from(MAX_U64) + 2));
        assert_eq!(MAX_U64, add(ZERO_U64, MAX_U128));

        assert_eq!(ONE_U64, add(ZERO_U64, ONE_U256));
        assert_eq!(ZERO_U64, add(MAX_U64, ONE_U256));
        assert_eq!(ONE_U64, add(ZERO_U64, U256::from(MAX_U64) + 2));
        assert_eq!(MAX_U64, add(ZERO_U64, MAX_U256));

        assert_eq!(ONE_U64, add(ZERO_U64, ONE_U512));
        assert_eq!(ZERO_U64, add(MAX_U64, ONE_U512));
        assert_eq!(ONE_U64, add(ZERO_U64, U512::from(MAX_U64) + 2));
        assert_eq!(MAX_U64, add(ZERO_U64, MAX_U512));

        // Adding to U128
        assert_eq!(ONE_U128, add(ZERO_U128, ONE_I32));
        assert_eq!(ZERO_U128, add(MAX_U128, ONE_I32));
        assert_eq!(ZERO_U128, add(ONE_U128, NEG_ONE_I32));
        assert_eq!(MAX_U128, add(ZERO_U128, NEG_ONE_I32));

        assert_eq!(ONE_U128, add(ZERO_U128, ONE_U64));
        assert_eq!(ZERO_U128, add(MAX_U128, ONE_U64));

        assert_eq!(ONE_U128, add(ZERO_U128, ONE_U128));
        assert_eq!(ZERO_U128, add(MAX_U128, ONE_U128));
        assert_eq!(MAX_U128 - 1, add(MAX_U128, MAX_U128));

        assert_eq!(ONE_U128, add(ZERO_U128, ONE_U256));
        assert_eq!(ZERO_U128, add(MAX_U128, ONE_U256));
        assert_eq!(
            ONE_U128,
            add(
                ZERO_U128,
                U256::from_dec_str(&MAX_U128.to_string()).unwrap() + 2,
            )
        );
        assert_eq!(MAX_U128, add(ZERO_U128, MAX_U256));

        assert_eq!(ONE_U128, add(ZERO_U128, ONE_U512));
        assert_eq!(ZERO_U128, add(MAX_U128, ONE_U512));
        assert_eq!(
            ONE_U128,
            add(
                ZERO_U128,
                U512::from_dec_str(&MAX_U128.to_string()).unwrap() + 2,
            )
        );
        assert_eq!(MAX_U128, add(ZERO_U128, MAX_U512));

        // Adding to U256
        assert_eq!(ONE_U256, add(ZERO_U256, ONE_I32));
        assert_eq!(ZERO_U256, add(MAX_U256, ONE_I32));
        assert_eq!(ZERO_U256, add(ONE_U256, NEG_ONE_I32));
        assert_eq!(MAX_U256, add(ZERO_U256, NEG_ONE_I32));

        assert_eq!(ONE_U256, add(ZERO_U256, ONE_U64));
        assert_eq!(ZERO_U256, add(MAX_U256, ONE_U64));

        assert_eq!(ONE_U256, add(ZERO_U256, ONE_U128));
        assert_eq!(ZERO_U256, add(MAX_U256, ONE_U128));

        assert_eq!(ONE_U256, add(ZERO_U256, ONE_U256));
        assert_eq!(ZERO_U256, add(MAX_U256, ONE_U256));
        assert_eq!(MAX_U256 - 1, add(MAX_U256, MAX_U256));

        assert_eq!(ONE_U256, add(ZERO_U256, ONE_U512));
        assert_eq!(ZERO_U256, add(MAX_U256, ONE_U512));
        assert_eq!(
            ONE_U256,
            add(
                ZERO_U256,
                U512::from_dec_str(&MAX_U256.to_string()).unwrap() + 2,
            )
        );
        assert_eq!(MAX_U256, add(ZERO_U256, MAX_U512));

        // Adding to U512
        assert_eq!(ONE_U512, add(ZERO_U512, ONE_I32));
        assert_eq!(ZERO_U512, add(MAX_U512, ONE_I32));
        assert_eq!(ZERO_U512, add(ONE_U512, NEG_ONE_I32));
        assert_eq!(MAX_U512, add(ZERO_U512, NEG_ONE_I32));

        assert_eq!(ONE_U512, add(ZERO_U512, ONE_U64));
        assert_eq!(ZERO_U512, add(MAX_U512, ONE_U64));

        assert_eq!(ONE_U512, add(ZERO_U512, ONE_U128));
        assert_eq!(ZERO_U512, add(MAX_U512, ONE_U128));

        assert_eq!(ONE_U512, add(ZERO_U512, ONE_U256));
        assert_eq!(ZERO_U512, add(MAX_U512, ONE_U256));

        assert_eq!(ONE_U512, add(ZERO_U512, ONE_U512));
        assert_eq!(ZERO_U512, add(MAX_U512, ONE_U512));
        assert_eq!(MAX_U512 - 1, add(MAX_U512, MAX_U512));
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..11 {
            let execution_result = TransformKindV2::random(rng);
            bytesrepr::test_serialization_roundtrip(&execution_result);
        }
    }
}
