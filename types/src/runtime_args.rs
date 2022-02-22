//! Home of RuntimeArgs for calling contracts

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::{collections::BTreeMap, string::String, vec::Vec};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, Error, FromBytes, ToBytes},
    CLType, CLTyped, CLValue, CLValueError, U512,
};
/// Named arguments to a contract.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct NamedArg(String, CLValue);

impl NamedArg {
    /// Returns a new `NamedArg`.
    pub fn new(name: String, value: CLValue) -> Self {
        NamedArg(name, value)
    }

    /// Returns the name of the named arg.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// Returns the value of the named arg.
    pub fn cl_value(&self) -> &CLValue {
        &self.1
    }

    /// Returns a mutable reference to the value of the named arg.
    pub fn cl_value_mut(&mut self) -> &mut CLValue {
        &mut self.1
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
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Serialize, Deserialize, Debug, Default)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct RuntimeArgs(Vec<NamedArg>);

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

    /// Gets the length of the collection.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the collection of arguments is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Inserts a new named argument into the collection.
    pub fn insert<K, V>(&mut self, key: K, value: V) -> Result<(), CLValueError>
    where
        K: Into<String>,
        V: CLTyped + ToBytes,
    {
        let cl_value = CLValue::from_t(value)?;
        self.0.push(NamedArg(key.into(), cl_value));
        Ok(())
    }

    /// Inserts a new named argument into the collection.
    pub fn insert_cl_value<K>(&mut self, key: K, cl_value: CLValue)
    where
        K: Into<String>,
    {
        self.0.push(NamedArg(key.into(), cl_value));
    }

    /// Returns all the values of the named args.
    pub fn to_values(&self) -> Vec<&CLValue> {
        self.0.iter().map(|NamedArg(_name, value)| value).collect()
    }

    /// Returns an iterator of references over all arguments in insertion order.
    pub fn named_args(&self) -> impl Iterator<Item = &NamedArg> {
        self.0.iter()
    }

    /// Returns an iterator of mutable references over all arguments in insertion order.
    pub fn named_args_mut(&mut self) -> impl Iterator<Item = &mut NamedArg> {
        self.0.iter_mut()
    }

    /// Returns the numeric value of `name` arg from the runtime arguments or defaults to
    /// 0 if that arg doesn't exist or is not an integer type.
    ///
    /// Supported [`CLType`]s for numeric conversions are U64, and U512.
    ///
    /// Returns an error if parsing the arg fails.
    pub fn try_get_number(&self, name: &str) -> Result<U512, CLValueError> {
        let amount_arg = match self.get(name) {
            None => return Ok(U512::zero()),
            Some(arg) => arg,
        };
        match amount_arg.cl_type() {
            CLType::U512 => amount_arg.clone().into_t::<U512>(),
            CLType::U64 => amount_arg.clone().into_t::<u64>().map(U512::from),
            _ => Ok(U512::zero()),
        }
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

impl From<RuntimeArgs> for BTreeMap<String, CLValue> {
    fn from(args: RuntimeArgs) -> BTreeMap<String, CLValue> {
        let mut map = BTreeMap::new();
        for named in args.0 {
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

    const ARG_AMOUNT: &str = "amount";

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

    #[test]
    fn try_get_number_should_work() {
        let mut args = RuntimeArgs::new();
        args.insert(ARG_AMOUNT, 0u64).expect("is ok");
        assert_eq!(args.try_get_number(ARG_AMOUNT).unwrap(), U512::zero());

        let mut args = RuntimeArgs::new();
        args.insert(ARG_AMOUNT, U512::zero()).expect("is ok");
        assert_eq!(args.try_get_number(ARG_AMOUNT).unwrap(), U512::zero());

        let args = RuntimeArgs::new();
        assert_eq!(args.try_get_number(ARG_AMOUNT).unwrap(), U512::zero());

        let hundred = 100u64;

        let mut args = RuntimeArgs::new();
        let input = U512::from(hundred);
        args.insert(ARG_AMOUNT, input).expect("is ok");
        assert_eq!(args.try_get_number(ARG_AMOUNT).unwrap(), input);

        let mut args = RuntimeArgs::new();
        args.insert(ARG_AMOUNT, hundred).expect("is ok");
        assert_eq!(
            args.try_get_number(ARG_AMOUNT).unwrap(),
            U512::from(hundred)
        );
    }

    #[test]
    fn try_get_number_should_return_zero_for_non_numeric_type() {
        let mut args = RuntimeArgs::new();
        args.insert(ARG_AMOUNT, "Non-numeric-string").unwrap();
        assert_eq!(
            args.try_get_number(ARG_AMOUNT).expect("should get amount"),
            U512::zero()
        );
    }

    #[test]
    fn try_get_number_should_return_zero_if_amount_is_missing() {
        let args = RuntimeArgs::new();
        assert_eq!(
            args.try_get_number(ARG_AMOUNT).expect("should get amount"),
            U512::zero()
        );
    }
}
