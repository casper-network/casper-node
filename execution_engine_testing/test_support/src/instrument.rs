//! A module for gathering instrumentation data.
use std::collections::BTreeMap;

use casper_types::PublicKey;
use num::BigInt;

/// Returns function name as a string.
///
/// This assumes this function is called from within macro.
#[allow(unused)]
pub fn get_function_name<T>(_: T) -> &'static str {
    let name = std::any::type_name::<T>();
    let mut tokens = name.rsplit("::");
    assert_eq!(tokens.next(), Some("f"));
    tokens.next().unwrap()
}

/// This macro returns function name at the call site.
#[macro_export]
macro_rules! function {
    () => {{
        fn f() {}
        $crate::instrument::get_function_name(f)
    }};
}

/// Holds a typed instrumented value.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstrumentedValue {
    /// Holds an integer of any size.
    Integer(BigInt),
    /// Holds a string.
    String(String),
    /// Holds a public key.
    PublicKey(PublicKey),
}

macro_rules! impl_from_for_integer {
    ($($type:ty)+) => {
        $(
        impl From<$type> for InstrumentedValue {
            fn from(value: $type) -> Self {
                Self::Integer(BigInt::from(value))
            }
        }
        )+
    }
}

impl_from_for_integer! {
    i8 u8 i16 u16 i32 u32 i64 u64 i128 u128 isize usize
}

impl From<String> for InstrumentedValue {
    fn from(value: String) -> Self {
        InstrumentedValue::String(value)
    }
}

impl<'a> From<&'a str> for InstrumentedValue {
    fn from(value: &'a str) -> Self {
        InstrumentedValue::String(value.to_owned())
    }
}

impl From<PublicKey> for InstrumentedValue {
    fn from(v: PublicKey) -> Self {
        Self::PublicKey(v)
    }
}

/// A container of instrumented value.
pub type InstrumentedValues = BTreeMap<&'static str, InstrumentedValue>;

/// Instrumented data.
pub struct Instrumented {
    pub(crate) module_name: &'static str,
    pub(crate) file: &'static str,
    pub(crate) line: u32,
    pub(crate) function: &'static str,
    pub(crate) data: InstrumentedValues,
}

impl Instrumented {
    /// Create a new instance of instrumentation info.
    pub fn new(
        module_name: &'static str,
        file: &'static str,
        line: u32,
        function: &'static str,
        data: InstrumentedValues,
    ) -> Self {
        Self {
            module_name,
            file,
            line,
            function,
            data,
        }
    }

    /// Adds extra data key-value pair to the instrumented values container.
    pub fn with_data<T: Into<InstrumentedValue>>(
        mut self,
        key: &'static str,
        value: T,
    ) -> Instrumented {
        self.data.insert(key, value.into());
        self
    }
}

/// Gathers instrumentation data and returns instance of [`Instrumented`] populated with current
/// function name, module, line number, and properties.
#[macro_export]
macro_rules! instrumented {
    ( $($key:expr => $value:expr),* $(,)?) => {
        $crate::instrument::Instrumented::new(
            module_path!(),
            file!(),
            line!(),
            $crate::function!(),
            #[allow(unused_mut)]
            {
                let mut data = $crate::instrument::InstrumentedValues::new();
                $(
                    let instrumented_value = $crate::instrument::InstrumentedValue::from($value);
                    data.insert($key, instrumented_value);
                )*
                data
            }
        )
    };

    ( $($value:expr),* $(,)? ) => {
        $crate::instrumented!($(stringify!($value) => $value,)*)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter::FromIterator;

    fn foo_function_name_2_named(param1: u64, param2: String) -> Instrumented {
        instrumented! { "param1" => param1, "param2" => param2, }
    }

    fn foo_function_name_0() -> Instrumented {
        instrumented!()
    }

    #[test]
    fn instrumented_values() {
        let map_like = instrumented!( "a" => 1u32, "b" => "xyz" );
        assert_eq!(
            map_like.data,
            BTreeMap::from_iter([
                ("a", InstrumentedValue::from(1u32)),
                ("b", InstrumentedValue::from("xyz".to_string()))
            ])
        );

        let i = 1u32;
        let j = "xyz";
        let set_like = instrumented!(i, j);
        assert_eq!(
            set_like.data,
            BTreeMap::from_iter([
                ("i", InstrumentedValue::from(1u32)),
                ("j", InstrumentedValue::from("xyz".to_string()))
            ])
        );
    }

    #[test]
    fn test_two_params() {
        let instrumented = foo_function_name_2_named(123456789, "foo".to_string());
        assert_eq!(
            instrumented.module_name,
            "casper_engine_test_support::instrument::tests"
        );
        assert_eq!(
            instrumented.file,
            "execution_engine_testing/test_support/src/instrument.rs"
        );
        assert!(instrumented.line > 0 && instrumented.line < 200);
        assert_eq!(instrumented.function, "foo_function_name_2_named");
        assert_eq!(
            instrumented.data.get("param1"),
            Some(&InstrumentedValue::from(123456789u64))
        );
        assert_eq!(
            instrumented.data.get("param2"),
            Some(&InstrumentedValue::from("foo"))
        );
    }

    #[test]
    fn test_zero_params() {
        let instrumented = foo_function_name_0();
        assert_eq!(
            instrumented.module_name,
            "casper_engine_test_support::instrument::tests"
        );
        assert_eq!(
            instrumented.file,
            "execution_engine_testing/test_support/src/instrument.rs"
        );
        assert!(instrumented.line > 0 && instrumented.line < 200);
        assert_eq!(instrumented.function, "foo_function_name_0");
        assert!(instrumented.data.is_empty());
    }
}
