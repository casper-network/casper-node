use crate::{
    bytesrepr::{self, Error, FromBytes},
    CLType, CLValue, Key, PublicKey, URef, U128, U256, U512,
};
use alloc::{string::String, vec, vec::Vec};
use serde::Serialize;
use serde_json::{json, Value};

type ToJSONResult<'a> = Result<(Value, &'a [u8]), Error>;

/// Returns a best-effort attempt to convert the `CLValue` into a meaningful JSON value.
pub fn cl_value_to_json(cl_value: &CLValue) -> Option<Value> {
    match to_json(&cl_value.cl_type(), cl_value.inner_bytes()) {
        Ok((value, rem)) => {
            if rem.is_empty() {
                Some(value)
            } else {
                None
            }
        }
        Err(_err) => None,
    }
}

fn to_json<'a>(cl_type: &'a CLType, bytes: &'a [u8]) -> ToJSONResult<'a> {
    match cl_type {
        CLType::Unit => simple_type_to_json::<()>(bytes),
        CLType::Bool => simple_type_to_json::<bool>(bytes),
        CLType::I32 => simple_type_to_json::<i32>(bytes),
        CLType::I64 => simple_type_to_json::<i64>(bytes),
        CLType::U8 => simple_type_to_json::<u8>(bytes),
        CLType::U32 => simple_type_to_json::<u32>(bytes),
        CLType::U64 => simple_type_to_json::<u64>(bytes),
        CLType::U128 => simple_type_to_json::<U128>(bytes),
        CLType::U256 => simple_type_to_json::<U256>(bytes),
        CLType::U512 => simple_type_to_json::<U512>(bytes),
        CLType::String => simple_type_to_json::<String>(bytes),
        CLType::Key => simple_type_to_json::<Key>(bytes),
        CLType::URef => simple_type_to_json::<URef>(bytes),
        CLType::PublicKey => simple_type_to_json::<PublicKey>(bytes),
        CLType::Option(inner_cl_type) => {
            let (variant, rem) = u8::from_bytes(bytes)?;
            match variant {
                0 => Ok((Value::Null, rem)),
                1 => Ok(to_json(inner_cl_type, rem)?),
                _ => Err(Error::Formatting),
            }
        }
        CLType::List(inner_cl_type) => {
            let (count, mut stream) = u32::from_bytes(bytes)?;
            let mut result: Vec<Value> = Vec::new();
            for _ in 0..count {
                let (value, rem) = to_json(inner_cl_type, &stream)?;
                result.push(value);
                stream = rem;
            }
            Ok((json!(result), stream))
        }
        CLType::ByteArray(length) => {
            let (bytes, rem) = bytesrepr::safe_split_at(bytes, *length as usize)?;
            Ok((json![bytes], rem))
        }
        CLType::Result { ok, err } => {
            let (variant, rem) = u8::from_bytes(bytes)?;
            match variant {
                0 => {
                    let (value, rem) = to_json(err, rem)?;
                    Ok((json!({ "error": value }), rem))
                }
                1 => {
                    let (value, rem) = to_json(ok, rem)?;
                    Ok((json!({ "ok": value }), rem))
                }
                _ => Err(Error::Formatting),
            }
        }
        CLType::Map { key, value } => {
            let (num_keys, mut stream) = u32::from_bytes(bytes).unwrap();
            let mut result: Vec<Value> = Vec::new();
            for _ in 0..num_keys {
                let (k, rem) = to_json(key, stream)?;
                let (v, rem) = to_json(value, rem)?;
                result.push(json!({"key": k, "value": v}));
                stream = rem;
            }
            Ok((json!(result), stream))
        }
        CLType::Tuple1(arr) => {
            let (t1, remainder) = to_json(&arr[0], bytes)?;
            Ok((json!(vec![t1]), remainder))
        }
        CLType::Tuple2(arr) => {
            let (t1, remainder) = to_json(&arr[0], bytes)?;
            let (t2, remainder) = to_json(&arr[1], remainder)?;
            Ok((json!(vec![t1, t2]), remainder))
        }
        CLType::Tuple3(arr) => {
            let (t1, remainder) = to_json(&arr[0], bytes)?;
            let (t2, remainder) = to_json(&arr[1], remainder)?;
            let (t3, remainder) = to_json(&arr[2], remainder)?;
            Ok((json!([t1, t2, t3]), remainder))
        }
        CLType::Any => Err(Error::Formatting),
    }
}

fn simple_type_to_json<T: FromBytes + Serialize>(bytes: &[u8]) -> ToJSONResult {
    let (value, rem) = T::from_bytes(bytes)?;
    Ok((json!(value), rem))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr::ToBytes, AsymmetricType, CLTyped};
    use alloc::collections::BTreeMap;

    fn test_value<T: ToBytes + Serialize + Clone + CLTyped>(value: T) {
        let cl_value = CLValue::from_t(value.clone()).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!(value);
        assert_eq!(cl_value_as_json, expected);
    }

    #[test]
    fn test_list_of_ints_to_json_value() {
        test_value::<Vec<i32>>(vec![]);
        test_value(vec![10u32, 12u32]);
    }

    #[test]
    fn test_list_of_bools_to_json_value() {
        test_value(vec![true, false]);
    }

    #[test]
    fn test_list_of_string_to_json_value() {
        test_value(vec!["rust", "python"]);
    }

    #[test]
    fn test_list_of_public_keys_to_json_value() {
        let a = PublicKey::secp256k1([3; PublicKey::SECP256K1_LENGTH]).unwrap();
        let b = PublicKey::ed25519([3; PublicKey::ED25519_LENGTH]).unwrap();
        let a_hex = a.to_hex();
        let b_hex = b.to_hex();
        let cl_value = CLValue::from_t(vec![a, b]).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!([a_hex, b_hex]);
        assert_eq!(cl_value_as_json, expected);
    }

    #[test]
    fn test_list_of_list_of_public_keys_to_json_value() {
        let a = PublicKey::secp256k1([3; PublicKey::SECP256K1_LENGTH]).unwrap();
        let b = PublicKey::ed25519([3; PublicKey::ED25519_LENGTH]).unwrap();
        let c = PublicKey::ed25519([6; PublicKey::ED25519_LENGTH]).unwrap();
        let a_hex = a.to_hex();
        let b_hex = b.to_hex();
        let c_hex = c.to_hex();
        let cl_value = CLValue::from_t(vec![vec![a, b], vec![c]]).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!([[a_hex, b_hex], [c_hex]]);
        assert_eq!(cl_value_as_json, expected);
    }

    #[test]
    fn test_map_of_string_to_list_of_ints_to_json_value() {
        let key1 = String::from("first");
        let key2 = String::from("second");
        let value1 = vec![];
        let value2 = vec![1, 2, 3];
        let mut map: BTreeMap<String, Vec<i32>> = BTreeMap::new();
        map.insert(key1.clone(), value1.clone());
        map.insert(key2.clone(), value2.clone());
        let cl_value = CLValue::from_t(map).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!([
            { "key": key1, "value": value1 },
            { "key": key2, "value": value2 }
        ]);
        assert_eq!(cl_value_as_json, expected);
    }

    #[test]
    fn test_option_some_of_lists_to_json_value() {
        let list = vec![1, 2, 3];
        let cl_value = CLValue::from_t(Some(list.clone())).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!(list);
        assert_eq!(cl_value_as_json, expected);
    }

    #[test]
    fn test_option_none_to_json_value() {
        let cl_value = CLValue::from_t::<Option<Vec<i32>>>(None).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = Value::Null;
        assert_eq!(cl_value_as_json, expected);
    }

    #[test]
    fn test_bytes_of_array_to_json_value() {
        test_value([]);
        test_value([1u8]);
        test_value([1u8, 2u8]);
    }

    #[test]
    fn test_result_ok_to_json_value() {
        let list = vec![1, 2, 3];
        let value: Result<Vec<i32>, String> = Ok(list.clone());
        let cl_value = CLValue::from_t(value).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!({ "ok": list });
        assert_eq!(cl_value_as_json, expected);
    }

    #[test]
    fn test_result_error_to_json_value() {
        let string = String::from("Upsss");
        let value: Result<Vec<i32>, String> = Err(string.clone());
        let cl_value = CLValue::from_t(value).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!({ "error": string });
        assert_eq!(cl_value_as_json, expected);
    }

    #[test]
    fn test_tuples_to_json_value() {
        let v1 = String::from("Hello");
        let v2 = vec![1, 2, 3];
        let v3 = 1u8;

        // Test Tuple1.
        let t1 = (v1.clone(),);
        let cl_value = CLValue::from_t(t1).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!([v1]);
        assert_eq!(cl_value_as_json, expected);

        // Test Tuple2.
        let t2 = (v1.clone(), v2.clone());
        let cl_value = CLValue::from_t(t2).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!([v1, v2]);
        assert_eq!(cl_value_as_json, expected);

        // Test Tuple3.
        let t3 = (v1.clone(), v2.clone(), v3);
        let cl_value = CLValue::from_t(t3).unwrap();
        let cl_value_as_json: Value = cl_value_to_json(&cl_value).unwrap();
        let expected = json!([v1, v2, v3]);
        assert_eq!(cl_value_as_json, expected);
    }
}
