//! Supported `CLType` and `CLValue` parsing and validation.

use std::{result::Result as StdResult, str::FromStr};

use casper_node::crypto::asymmetric_key::PublicKey as NodePublicKey;
use casper_types::{
    account::AccountHash, CLType, CLTyped, CLValue, Key, PublicKey, URef, U128, U256, U512,
};

use crate::error::{Error, Result};

/// Parse a `CLType` from `&str`.
pub fn parse(strval: &str) -> StdResult<CLType, ()> {
    let supported_types = supported_cl_types();
    let cl_type = match strval.to_lowercase() {
        t if t == supported_types[0].0 => supported_types[0].1.clone(),
        t if t == supported_types[1].0 => supported_types[1].1.clone(),
        t if t == supported_types[2].0 => supported_types[2].1.clone(),
        t if t == supported_types[3].0 => supported_types[3].1.clone(),
        t if t == supported_types[4].0 => supported_types[4].1.clone(),
        t if t == supported_types[5].0 => supported_types[5].1.clone(),
        t if t == supported_types[6].0 => supported_types[6].1.clone(),
        t if t == supported_types[7].0 => supported_types[7].1.clone(),
        t if t == supported_types[8].0 => supported_types[8].1.clone(),
        t if t == supported_types[9].0 => supported_types[9].1.clone(),
        t if t == supported_types[10].0 => supported_types[10].1.clone(),
        t if t == supported_types[11].0 => supported_types[11].1.clone(),
        t if t == supported_types[12].0 => supported_types[12].1.clone(),
        t if t == supported_types[13].0 => supported_types[13].1.clone(),
        t if t == supported_types[14].0 => supported_types[14].1.clone(),
        _ => return Err(()),
    };
    Ok(cl_type)
}

pub(crate) fn supported_cl_types() -> Vec<(&'static str, CLType)> {
    vec![
        ("bool", CLType::Bool),
        ("i32", CLType::I32),
        ("i64", CLType::I64),
        ("u8", CLType::U8),
        ("u32", CLType::U32),
        ("u64", CLType::U64),
        ("u128", CLType::U128),
        ("u256", CLType::U256),
        ("u512", CLType::U512),
        ("unit", CLType::Unit),
        ("string", CLType::String),
        ("key", CLType::Key),
        ("account_hash", AccountHash::cl_type()),
        ("uref", CLType::URef),
        ("public_key", CLType::PublicKey),
    ]
}

/// Returns a list of `CLTypes` supported by casper_client.
pub fn supported_cl_type_list() -> String {
    let mut msg = String::new();
    let supported_types = supported_cl_types();
    for (index, item) in supported_types.iter().map(|(name, _)| name).enumerate() {
        msg.push_str(item);
        if index < supported_types.len() - 1 {
            msg.push_str(", ")
        }
    }
    msg
}

/// Parse a `CLValue` given a `CLType` and a `value`, 
pub fn parse_value(cl_type: CLType, value: &str) -> Result<CLValue> {
    let cl_value = match cl_type {
        CLType::Bool => match value.to_lowercase().as_str() {
            "true" | "t" => CLValue::from_t(true).expect("should be true"),
            "false" | "f" => CLValue::from_t(false).expect("should be true"),
            invalid => {
                return Err(Error::InvalidCLValue(format!(
                    "can't parse {} as a bool.  Should be 'true' or 'false'",
                    invalid
                )))
            }
        },
        CLType::I32 => {
            let x = i32::from_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as i32: {}", value, error))
            })?;
            CLValue::from_t(x).unwrap()
        }
        CLType::I64 => {
            let x = i64::from_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as i64: {}", value, error))
            })?;
            CLValue::from_t(x).unwrap()
        }
        CLType::U8 => {
            let x = u8::from_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as u8: {}", value, error))
            })?;
            CLValue::from_t(x).unwrap()
        }
        CLType::U32 => {
            let x = u32::from_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as u32: {}", value, error))
            })?;
            CLValue::from_t(x).unwrap()
        }
        CLType::U64 => {
            let x = u64::from_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as u64: {}", value, error))
            })?;
            CLValue::from_t(x).unwrap()
        }
        CLType::U128 => {
            let x = U128::from_dec_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as U128: {}", value, error))
            })?;
            CLValue::from_t(x).unwrap()
        }
        CLType::U256 => {
            let x = U256::from_dec_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as U256: {}", value, error))
            })?;
            CLValue::from_t(x).unwrap()
        }
        CLType::U512 => {
            let x = U512::from_dec_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as U512: {}", value, error))
            })?;
            CLValue::from_t(x).unwrap()
        }
        CLType::Unit => {
            if !value.is_empty() {
                return Err(Error::InvalidCLValue(format!(
                    "can't parse {} as unit.  Should be ''",
                    value
                )));
            }
            CLValue::from_t(()).unwrap()
        }
        CLType::String => CLValue::from_t(value).unwrap(),
        CLType::Key => {
            let key = Key::from_formatted_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as Key: {:?}", value, error))
            })?;
            CLValue::from_t(key).unwrap()
        }
        CLType::FixedList(ty, 32) => match *ty {
            CLType::U8 => {
                let account_hash = AccountHash::from_formatted_str(value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as AccountHash: {:?}",
                        value, error
                    ))
                })?;
                CLValue::from_t(account_hash).unwrap()
            }
            _ => unreachable!(),
        },
        CLType::URef => {
            let uref = URef::from_formatted_str(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as URef: {:?}", value, error))
            })?;
            CLValue::from_t(uref).unwrap()
        }
        CLType::PublicKey => {
            let pub_key = NodePublicKey::from_hex(value).map_err(|error| {
                Error::InvalidCLValue(format!("can't parse {} as PublicKey: {:?}", value, error))
            })?;
            CLValue::from_t(PublicKey::from(pub_key)).unwrap()
        }
        _ => unreachable!(),
    };
    Ok(cl_value)
}
