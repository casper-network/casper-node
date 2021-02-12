//! Home of RuntimeArgs for calling contracts

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use datasize::DataSize;
#[cfg(feature = "std")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, Error, FromBytes, ToBytes},
    CLTyped, CLValue, CLValueError,
};

/// Named arguments to a contract
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Serialize, Deserialize, Debug, DataSize)]
#[cfg_attr(feature = "std", derive(JsonSchema))]
pub struct NamedArg(#[data_size(skip)] String, CLValue);

impl NamedArg {
    /// ctor
    pub fn new(name: String, value: CLValue) -> Self {
        NamedArg(name, value)
    }
    /// returns `name`
    pub fn name(&self) -> &str {
        &self.0
    }
    /// returns `value`
    pub fn cl_value(&self) -> &CLValue {
        &self.1
    }
}

impl From<(String, CLValue)> for NamedArg {
    fn from((name, value): (String, CLValue)) -> NamedArg {
        NamedArg(name, value)
    }
}

impl ToBytes for NamedArg {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length() + self.1.serialized_length()
    }
}

impl FromBytes for NamedArg {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (name, remainder) = String::from_bytes(bytes)?;
        let (cl_value, remainder) = CLValue::from_bytes(remainder)?;
        Ok((NamedArg(name, cl_value), remainder))
    }
}

/// Represents a collection of arguments passed to a smart contract.
#[derive(
    PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Serialize, Deserialize, Debug, Default, DataSize,
)]
#[cfg_attr(feature = "std", derive(JsonSchema))]
pub struct RuntimeArgs(#[data_size(skip)] Vec<NamedArg>);

impl RuntimeArgs {
    /// Create an empty [`RuntimeArgs`] instance.
    pub fn new() -> RuntimeArgs {
        RuntimeArgs::default()
    }

    /// A wrapper that lets you easily and safely create runtime arguments.
    ///
    /// This method is useful when you have to construct a [`RuntimeArgs`] with multiple entries,
    /// but error handling at given call site would require to have a match statement for each
    /// [`RuntimeArgs::insert`] call. With this method you can use ? operator inside the closure and
    /// then handle single result. When `try_block` will be stabilized this method could be
    /// deprecated in favor of using those blocks.
    pub fn try_new<F>(func: F) -> Result<RuntimeArgs, CLValueError>
    where
        F: FnOnce(&mut RuntimeArgs) -> Result<(), CLValueError>,
    {
        let mut runtime_args = RuntimeArgs::new();
        func(&mut runtime_args)?;
        Ok(runtime_args)
    }

    /// Gets an argument by its name.
    pub fn get(&self, name: &str) -> Option<&CLValue> {
        self.0.iter().find_map(|NamedArg(named_name, named_value)| {
            if named_name == name {
                Some(named_value)
            } else {
                None
            }
        })
    }

    /// Get length of the collection.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Check if collection of arguments is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Insert new named argument into the collection.
    pub fn insert<K, V>(&mut self, key: K, value: V) -> Result<(), CLValueError>
    where
        K: Into<String>,
        V: CLTyped + ToBytes,
    {
        let cl_value = CLValue::from_t(value)?;
        self.0.push(NamedArg(key.into(), cl_value));
        Ok(())
    }

    /// Insert new named argument into the collection.
    pub fn insert_cl_value<K>(&mut self, key: K, cl_value: CLValue)
    where
        K: Into<String>,
    {
        self.0.push(NamedArg(key.into(), cl_value));
    }

    /// Returns values held regardless of the variant.
    pub fn to_values(&self) -> Vec<&CLValue> {
        self.0.iter().map(|NamedArg(_name, value)| value).collect()
    }
}

impl From<Vec<NamedArg>> for RuntimeArgs {
    fn from(values: Vec<NamedArg>) -> Self {
        RuntimeArgs(values)
    }
}

impl From<BTreeMap<String, CLValue>> for RuntimeArgs {
    fn from(cl_values: BTreeMap<String, CLValue>) -> RuntimeArgs {
        RuntimeArgs(cl_values.into_iter().map(NamedArg::from).collect())
    }
}

impl Into<BTreeMap<String, CLValue>> for RuntimeArgs {
    fn into(self) -> BTreeMap<String, CLValue> {
        let mut map = BTreeMap::new();
        for named in self.0 {
            map.insert(named.0, named.1);
        }
        map
    }
}

impl ToBytes for RuntimeArgs {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for RuntimeArgs {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (args, remainder) = Vec::<NamedArg>::from_bytes(bytes)?;
        Ok((RuntimeArgs(args), remainder))
    }
}

/// Macro that makes it easier to construct named arguments.
///
/// NOTE: This macro does not propagate possible errors that could occur while creating a
/// [`crate::CLValue`]. For such cases creating [`RuntimeArgs`] manually is recommended.
///
/// # Example usage
/// ```
/// use casper_types::{RuntimeArgs, runtime_args};
/// let _named_args = runtime_args! {
///   "foo" => 42,
///   "bar" => "Hello, world!"
/// };
/// ```
#[macro_export]
macro_rules! runtime_args {
    () => (RuntimeArgs::new());
    ( $($key:expr => $value:expr,)+ ) => (runtime_args!($($key => $value),+));
    ( $($key:expr => $value:expr),* ) => {
        {
            let mut named_args = RuntimeArgs::new();
            $(
                named_args.insert($key, $value).unwrap();
            )*
            named_args
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_args() {
        let arg1 = CLValue::from_t(1).unwrap();
        let arg2 = CLValue::from_t("Foo").unwrap();
        let arg3 = CLValue::from_t(Some(1)).unwrap();
        let args = {
            let mut map = BTreeMap::new();
            map.insert("bar".into(), arg2.clone());
            map.insert("foo".into(), arg1.clone());
            map.insert("qwer".into(), arg3.clone());
            map
        };
        let runtime_args = RuntimeArgs::from(args);
        assert_eq!(runtime_args.get("qwer"), Some(&arg3));
        assert_eq!(runtime_args.get("foo"), Some(&arg1));
        assert_eq!(runtime_args.get("bar"), Some(&arg2));
        assert_eq!(runtime_args.get("aaa"), None);

        // Ensure macro works

        let runtime_args_2 = runtime_args! {
            "bar" => "Foo",
            "foo" => 1i32,
            "qwer" => Some(1i32),
        };
        assert_eq!(runtime_args, runtime_args_2);
    }

    #[test]
    fn empty_macro() {
        assert_eq!(runtime_args! {}, RuntimeArgs::new());
    }

    #[test]
    fn btreemap_compat() {
        // This test assumes same serialization format as BTreeMap
        let runtime_args_1 = runtime_args! {
            "bar" => "Foo",
            "foo" => 1i32,
            "qwer" => Some(1i32),
        };
        let tagless = runtime_args_1.to_bytes().unwrap().to_vec();

        let mut runtime_args_2 = BTreeMap::new();
        runtime_args_2.insert(String::from("bar"), CLValue::from_t("Foo").unwrap());
        runtime_args_2.insert(String::from("foo"), CLValue::from_t(1i32).unwrap());
        runtime_args_2.insert(String::from("qwer"), CLValue::from_t(Some(1i32)).unwrap());

        assert_eq!(tagless, runtime_args_2.to_bytes().unwrap());
    }

    #[test]
    fn named_serialization_roundtrip() {
        let args = runtime_args! {
            "foo" => 1i32,
        };
        bytesrepr::test_serialization_roundtrip(&args);
    }

    #[test]
    fn should_create_args_with() {
        let res = RuntimeArgs::try_new(|runtime_args| {
            runtime_args.insert(String::from("foo"), 123)?;
            runtime_args.insert(String::from("bar"), 456)?;
            Ok(())
        });

        let expected = runtime_args! {
            "foo" => 123,
            "bar" => 456,
        };
        assert!(matches!(res, Ok(args) if expected == args));
    }
}
