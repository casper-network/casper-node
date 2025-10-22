use crate::prelude::{collections::BTreeMap, *};

use crate::{
    compat::types::{CLType, CLTyped, CLValue, U512},
    serializers::borsh::{io, BorshDeserialize, BorshSerialize},
};

pub type NamedArg<'a> = (&'a String, &'a CLValue);

#[derive(
    PartialEq, Eq, PartialOrd, Ord, Hash, Clone, BorshSerialize, BorshDeserialize, Debug, Default,
)]
pub struct RuntimeArgs(BTreeMap<String, CLValue>);

impl RuntimeArgs {
    pub const fn new() -> RuntimeArgs {
        RuntimeArgs(BTreeMap::new())
    }

    #[inline]
    pub fn get(&self, name: &str) -> Option<&CLValue> {
        self.0.get(name)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Inserts a new named argument into the collection.
    pub fn insert<K, V>(&mut self, key: K, value: V) -> io::Result<()>
    where
        K: Into<String>,
        V: CLTyped + BorshSerialize,
    {
        let cl_value = CLValue::from_t(value)?;
        self.insert_cl_value(key, cl_value);
        Ok(())
    }

    /// Inserts a new named argument into the collection.
    pub fn insert_cl_value<K>(&mut self, key: K, cl_value: CLValue)
    where
        K: Into<String>,
    {
        self.0.insert(key.into(), cl_value);
    }

    /// Returns all the values of the named args.
    pub fn to_values(&self) -> Vec<&CLValue> {
        self.0.values().collect()
    }

    /// Returns an iterator of references over all arguments in insertion order.
    pub fn named_args(&self) -> impl Iterator<Item = NamedArg> {
        self.0.iter()
    }

    /// Returns the numeric value of `name` arg from the runtime arguments or defaults to
    /// 0 if that arg doesn't exist or is not an integer type.
    ///
    /// Supported [`CLType`]s for numeric conversions are U64, and U512.
    ///
    /// Returns an error if parsing the arg fails.
    pub fn try_get_number(&self, name: &str) -> io::Result<U512> {
        let amount_arg = match self.get(name) {
            None => return Ok(U512::ZERO),
            Some(arg) => arg,
        };
        match amount_arg.cl_type() {
            CLType::U512 => amount_arg.clone().into_t::<U512>(),
            CLType::U64 => amount_arg.clone().into_t::<u64>().map(U512::from),
            _ => Ok(U512::ZERO),
        }
    }
}
