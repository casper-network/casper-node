//! Supported `CLType` and `CLValue` parsing and validation.

use std::{result::Result as StdResult, str::FromStr};

use casper_types::{
    account::AccountHash, bytesrepr::ToBytes, AsymmetricType, CLType, CLTyped, CLValue, Key,
    PublicKey, URef, U128, U256, U512,
};

use crate::error::{Error, Result};

/// Parse a `CLType` from `&str`.
pub(crate) fn parse(strval: &str) -> StdResult<CLType, ()> {
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
        t if t == supported_types[15].0 => supported_types[15].1.clone(),
        t if t == supported_types[16].0 => supported_types[16].1.clone(),
        t if t == supported_types[17].0 => supported_types[17].1.clone(),
        t if t == supported_types[18].0 => supported_types[18].1.clone(),
        t if t == supported_types[19].0 => supported_types[19].1.clone(),
        t if t == supported_types[20].0 => supported_types[20].1.clone(),
        t if t == supported_types[21].0 => supported_types[21].1.clone(),
        t if t == supported_types[22].0 => supported_types[22].1.clone(),
        t if t == supported_types[23].0 => supported_types[23].1.clone(),
        t if t == supported_types[24].0 => supported_types[24].1.clone(),
        t if t == supported_types[25].0 => supported_types[25].1.clone(),
        t if t == supported_types[26].0 => supported_types[26].1.clone(),
        t if t == supported_types[27].0 => supported_types[27].1.clone(),
        t if t == supported_types[28].0 => supported_types[28].1.clone(),
        t if t == supported_types[29].0 => supported_types[29].1.clone(),
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
        ("opt_bool", CLType::Option(Box::new(CLType::Bool))),
        ("opt_i32", CLType::Option(Box::new(CLType::I32))),
        ("opt_i64", CLType::Option(Box::new(CLType::I64))),
        ("opt_u8", CLType::Option(Box::new(CLType::U8))),
        ("opt_u32", CLType::Option(Box::new(CLType::U32))),
        ("opt_u64", CLType::Option(Box::new(CLType::U64))),
        ("opt_u128", CLType::Option(Box::new(CLType::U128))),
        ("opt_u256", CLType::Option(Box::new(CLType::U256))),
        ("opt_u512", CLType::Option(Box::new(CLType::U512))),
        ("opt_unit", CLType::Option(Box::new(CLType::Unit))),
        ("opt_string", CLType::Option(Box::new(CLType::String))),
        ("opt_key", CLType::Option(Box::new(CLType::Key))),
        (
            "opt_account_hash",
            CLType::Option(Box::new(AccountHash::cl_type())),
        ),
        ("opt_uref", CLType::Option(Box::new(CLType::URef))),
        (
            "opt_public_key",
            CLType::Option(Box::new(CLType::PublicKey)),
        ),
    ]
}

/// Functions for use in help commands.
pub mod help {
    use std::convert::TryFrom;

    use casper_types::{account::AccountHash, AccessRights, AsymmetricType, Key, PublicKey, URef};

    /// Returns a list of `CLType`s able to be passed as a string for use as payment code or session
    /// code args.
    pub fn supported_cl_type_list() -> String {
        let mut msg = String::new();
        let supported_types = super::supported_cl_types();
        for (index, item) in supported_types.iter().map(|(name, _)| name).enumerate() {
            msg.push_str(item);
            if index < supported_types.len() - 1 {
                msg.push_str(", ")
            }
        }
        msg
    }

    /// Returns a string listing examples of the format required when passing in payment code or
    /// session code args.
    pub fn supported_cl_type_examples() -> String {
        let bytes = (1..33).collect::<Vec<_>>();
        let array = <[u8; 32]>::try_from(bytes.as_ref()).unwrap();

        format!(
            r#""name_01:bool='false'"
"name_02:i32='-1'"
"name_03:i64='-2'"
"name_04:u8='3'"
"name_05:u32='4'"
"name_06:u64='5'"
"name_07:u128='6'"
"name_08:u256='7'"
"name_09:u512='8'"
"name_10:unit=''"
"name_11:string='a value'"
"key_account_name:key='{}'"
"key_hash_name:key='{}'"
"key_uref_name:key='{}'"
"account_hash_name:account_hash='{}'"
"uref_name:uref='{}'"
"public_key_name:public_key='{}'"

Optional values of all of these types can also be specified.
Prefix the type with "opt_" and use the term "null" without quotes to specify a None value:
"name_01:opt_bool='true'"       # Some(true)
"name_02:opt_bool='false'"      # Some(false)
"name_03:opt_bool=null"         # None
"name_04:opt_i32='-1'"          # Some(-1)
"name_05:opt_i32=null"          # None
"name_06:opt_unit=''"           # Some(())
"name_07:opt_unit=null"         # None
"name_08:opt_string='a value'"  # Some("a value".to_string())
"name_09:opt_string='null'"     # Some("null".to_string())
"name_10:opt_string=null"       # None
"#,
            Key::Account(AccountHash::new(array)).to_formatted_string(),
            Key::Hash(array).to_formatted_string(),
            Key::URef(URef::new(array, AccessRights::NONE)).to_formatted_string(),
            AccountHash::new(array).to_formatted_string(),
            URef::new(array, AccessRights::READ_ADD_WRITE).to_formatted_string(),
            PublicKey::from_hex(
                "0119bf44096984cdfe8541bac167dc3b96c85086aa30b6b6cb0c5c38ad703166e1"
            )
            .unwrap()
            .to_hex(),
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
enum OptionalStatus {
    Some,
    None,
    NotOptional,
}

/// Parses to a given CLValue taking into account whether the arg represents an optional type or
/// not.
fn parse_to_cl_value<T, F>(optional_status: OptionalStatus, parse: F) -> Result<CLValue>
where
    T: CLTyped + ToBytes,
    F: FnOnce() -> Result<T>,
{
    match optional_status {
        OptionalStatus::Some => CLValue::from_t(Some(parse()?)),
        OptionalStatus::None => CLValue::from_t::<Option<T>>(None),
        OptionalStatus::NotOptional => CLValue::from_t(parse()?),
    }
    .map_err(|error| {
        Error::InvalidCLValue(format!(
            "unable to parse cl value {:?} with optional_status {:?}",
            error, optional_status
        ))
    })
}

/// Returns a value built from a single arg which has been split into its constituent parts.
pub fn parts_to_cl_value(cl_type: CLType, value: &str) -> Result<CLValue> {
    let (cl_type_to_parse, optional_status, trimmed_value) = match cl_type {
        CLType::Option(inner_type) => {
            if value == "null" {
                (*inner_type, OptionalStatus::None, "")
            } else {
                (*inner_type, OptionalStatus::Some, value.trim_matches('\''))
            }
        }
        _ => (
            cl_type,
            OptionalStatus::NotOptional,
            value.trim_matches('\''),
        ),
    };

    if value == trimmed_value {
        return Err(Error::InvalidCLValue(format!(
            "value in simple arg should be surrounded by single quotes unless it's a null \
                   optional value (value passed: {})",
            value
        )));
    }

    match cl_type_to_parse {
        CLType::Bool => {
            let parse = || match trimmed_value.to_lowercase().as_str() {
                "true" | "t" => Ok(true),
                "false" | "f" => Ok(false),
                invalid => Err(Error::InvalidCLValue(format!(
                    "can't parse {} as a bool. Should be 'true' or 'false'",
                    invalid
                ))),
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::I32 => {
            let parse = || {
                i32::from_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!("can't parse {} as i32: {}", value, error))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::I64 => {
            let parse = || {
                i64::from_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as i64: {}",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::U8 => {
            let parse = || {
                u8::from_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!("can't parse {} as u8: {}", trimmed_value, error))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::U32 => {
            let parse = || {
                u32::from_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as u32: {}",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::U64 => {
            let parse = || {
                u64::from_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as u64: {}",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::U128 => {
            let parse = || {
                U128::from_dec_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as U128: {}",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::U256 => {
            let parse = || {
                U256::from_dec_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as U256: {}",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::U512 => {
            let parse = || {
                U512::from_dec_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as U512: {}",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::Unit => {
            let parse = || {
                if !trimmed_value.is_empty() {
                    return Err(Error::InvalidCLValue(format!(
                        "can't parse {} as unit. Should be ''",
                        trimmed_value
                    )));
                }
                Ok(())
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::String => {
            let parse = || Ok(trimmed_value.to_string());
            parse_to_cl_value(optional_status, parse)
        }
        CLType::Key => {
            let parse = || {
                Key::from_formatted_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as Key: {}",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::ByteArray(32) => {
            let parse = || {
                AccountHash::from_formatted_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as AccountHash: {:?}.\
                        AccountHash type values should start with 'account-hash-' prefix.",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::URef => {
            let parse = || {
                URef::from_formatted_str(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as URef: {:?}. \
                        URef type values should start with 'uref-' prefix.",
                        trimmed_value, error
                    ))
                })
            };
            parse_to_cl_value(optional_status, parse)
        }
        CLType::PublicKey => {
            let parse = || {
                let pub_key = PublicKey::from_hex(trimmed_value).map_err(|error| {
                    Error::InvalidCLValue(format!(
                        "can't parse {} as PublicKey: {:?}",
                        trimmed_value, error
                    ))
                })?;
                Ok(pub_key)
            };
            parse_to_cl_value(optional_status, parse)
        }
        _ => unreachable!(),
    }
}
