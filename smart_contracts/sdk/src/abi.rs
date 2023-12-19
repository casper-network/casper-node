use impl_trait_for_tuples::impl_for_tuples;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Declaration {
    /// Referenced by name in the ABI.
    Ref(String),
    Type(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub discriminant: u64,
    pub body: Definition,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StructField {
    pub name: String,
    pub body: Definition,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Definition {
    /// Primitive type.
    ///
    /// Examples: u64, i32, f32, String, bool, etc.
    Type(String),
    /// A mapping.
    ///
    /// Example Rust types: BTreeMap<K, V>.
    Mapping {
        key: Box<Definition>,
        value: Box<Definition>,
    },
    /// Arbitrary sequence of values.
    ///
    /// Example Rust types: Vec<T>, &[T], [T; N], Box<[T]>
    Sequence {
        /// If length is known, then it specifies that this definition should be be represented as
        /// an array of a fixed size.
        length: Option<u32>,
        def: Box<Definition>,
    },
    /// A tuple of multiple values of various types.
    ///
    /// Can be also used to represent a heterogeneous list.
    Tuple {
        items: Vec<Definition>,
    },
    Enum {
        items: Vec<EnumVariant>,
    },
    Struct {
        items: Vec<StructField>,
    },
}

impl Definition {
    pub fn unit() -> Self {
        // Empty struct should be equivalent to `()` in Rust in other languages.
        Definition::Tuple { items: Vec::new() }
    }
}

pub trait CasperABI {
    fn definition() -> Definition;
}

macro_rules! impl_abi_for_types {
    // Accepts following syntax: impl_abi_for_types(u8, u16, u32, u64, String => "string", f32, f64)
    ($($ty:ty $(=> $name:expr)?,)* ) => {
        $(
            impl_abi_for_types!(@impl $ty $(=> $name)?);
        )*
    };

    (@impl $ty:ty ) => {
       impl_abi_for_types!(@impl $ty => stringify!($ty));
    };

    (@impl $ty:ty => $name:expr ) => {
        impl CasperABI for $ty {
            fn definition() -> Definition {
                Definition::Type($name.into())
            }
        }
    };
}

impl CasperABI for () {
    fn definition() -> Definition {
        Definition::unit()
    }
}

impl_abi_for_types!(
    bool,
    u8, u16, u32, u64,
    i8, i16, i32, i64,
    f32, f64,
    String => "string",
    &str => "string",
);

#[impl_for_tuples(1, 12)]
impl CasperABI for Tuple {
    fn definition() -> Definition {
        let items: Vec<Definition> =
            <[_]>::into_vec(Box::new([for_tuples!( #( Tuple::definition() ),* )]));
        Definition::Tuple { items }
    }
}

impl<T: CasperABI, E: CasperABI> CasperABI for Result<T, E> {
    fn definition() -> Definition {
        Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "Ok".to_string(),
                    discriminant: 0,
                    body: T::definition(),
                },
                EnumVariant {
                    name: "Err".to_string(),
                    discriminant: 1,
                    body: E::definition(),
                },
            ],
        }
    }
}

impl<T: CasperABI> CasperABI for Vec<T> {
    fn definition() -> Definition {
        Definition::Sequence {
            length: None,
            def: Box::new(T::definition()),
        }
    }
}

impl<T: CasperABI, const N: usize> CasperABI for [T; N] {
    fn definition() -> Definition {
        Definition::Sequence {
            length: Some(N.try_into().expect("N is too big")),
            def: Box::new(T::definition()),
        }
    }
}

impl<K: CasperABI, V: CasperABI> CasperABI for BTreeMap<K, V> {
    fn definition() -> Definition {
        Definition::Mapping {
            key: Box::new(K::definition()),
            value: Box::new(V::definition()),
        }
    }
}
