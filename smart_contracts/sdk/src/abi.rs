use core::mem;

use crate::{
    common::type_uid::{TypeUid, Uid},
    prelude::{
        collections,
        collections::{BTreeMap, BTreeSet, HashMap, LinkedList},
        str::FromStr,
    },
};
use impl_trait_for_tuples::impl_for_tuples;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub discriminant: u64,
    pub decl: Declaration,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub struct StructField {
    pub name: String,
    pub decl: Declaration,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum Primitive {
    Char,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    U128,
    I128,
    F32,
    F64,
    Bool,
}

impl FromStr for Primitive {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Primitive::*;
        match s {
            "Char" => Ok(Char),
            "U8" => Ok(U8),
            "I8" => Ok(I8),
            "U16" => Ok(U16),
            "I16" => Ok(I16),
            "U32" => Ok(U32),
            "I32" => Ok(I32),
            "U64" => Ok(U64),
            "I64" => Ok(I64),
            "U128" => Ok(U128),
            "I128" => Ok(I128),
            "F32" => Ok(F32),
            "F64" => Ok(F64),
            "Bool" => Ok(Bool),
            _ => Err("Unknown primitive type"),
        }
    }
}

pub trait Keyable {
    const PRIMITIVE: Primitive;
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
#[serde(tag = "type")]
pub enum TypeDef {
    /// Primitive type.
    ///
    /// Examples: u64, i32, f32, bool, etc
    Primitive(Primitive),
    /// A mapping.
    ///
    /// Example Rust types: BTreeMap<K, V>.
    Mapping {
        key: Declaration,
        value: Declaration,
    },
    /// Arbitrary sequence of values.
    ///
    /// Example Rust types: `Vec<T>`, `&[T]`, `[T; N]`, `Box<[T]>`
    Sequence {
        /// If length is known, then it specifies that this definition should be be represented as
        /// an array of a fixed size.
        decl: Declaration,
    },
    FixedSequence {
        /// If length is known, then it specifies that this definition should be be represented as
        /// an array of a fixed size.
        length: u32, // None -> Vec<T> Some(N) [T; N]
        decl: Declaration,
    },
    /// A tuple of multiple values of various types.
    ///
    /// Can be also used to represent a heterogeneous list.
    Tuple {
        items: Vec<Declaration>,
    },
    Enum {
        items: Vec<EnumVariant>,
    },
    Struct {
        items: Vec<StructField>,
    },
}

/// Represents a unique identifier for a type definition in the ABI.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UidJsonValue(Uid);

impl Serialize for UidJsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let hex_string = format!("0x{:016x}", self.0);
        serializer.serialize_str(&hex_string)
    }
}

impl<'de> Deserialize<'de> for UidJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let hex_str = s.strip_prefix("0x").unwrap_or(&s);
        let uid = u64::from_str_radix(hex_str, 16)
            .map_err(|e| serde::de::Error::custom(format!("Invalid hex string: {}", e)))?;
        Ok(UidJsonValue(Uid::from(uid)))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
pub struct Definition {
    /// The type definition.
    #[serde(flatten)]
    pub type_def: TypeDef,
    pub uid: UidJsonValue, // Unique identifier for the type definition.
}

impl TypeDef {
    pub fn unit() -> Self {
        // Empty struct should be equivalent to `()` in Rust in other languages.
        Self::Tuple { items: Vec::new() }
    }

    pub fn as_struct(&self) -> Option<&[StructField]> {
        if let Self::Struct { items } = self {
            Some(items.as_slice())
        } else {
            None
        }
    }

    pub fn as_enum(&self) -> Option<&[EnumVariant]> {
        if let Self::Enum { items } = self {
            Some(items.as_slice())
        } else {
            None
        }
    }

    pub fn as_tuple(&self) -> Option<&[Declaration]> {
        if let Self::Tuple { items } = self {
            Some(items.as_slice())
        } else {
            None
        }
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Definitions(BTreeMap<Declaration, Definition>);

impl Definitions {
    pub fn populate_one<T: CasperABI + TypeUid>(&mut self) {
        T::populate_definitions(self);

        let decl = T::declaration();
        let type_def = T::type_def();

        let def = Definition {
            type_def,
            uid: UidJsonValue(T::UID),
        };

        self.populate_custom(decl, def);
    }

    pub fn populate_custom(&mut self, decl: Declaration, def: Definition) {
        let previous = self.0.insert(decl.clone(), def.clone());
        if previous.is_some() && previous != Some(def.clone()) {
            panic!("Type {decl} has multiple definitions ({previous:?} != {def:?}).");
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Declaration, &Definition)> {
        self.0.iter()
    }

    pub fn get(&self, decl: &str) -> Option<&Definition> {
        self.0.get(decl)
    }

    pub fn first(&self) -> Option<(&Declaration, &Definition)> {
        self.0.iter().next()
    }

    /// Returns true if the given declaration has a definition in this set.
    pub fn has_definition(&self, decl: &Declaration) -> bool {
        self.0.contains_key(decl)
    }
}

impl IntoIterator for Definitions {
    type Item = (Declaration, Definition);
    type IntoIter = collections::btree_map::IntoIter<Declaration, Definition>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

pub type Declaration = String;

pub trait CasperABI: TypeUid {
    fn populate_definitions(definitions: &mut Definitions);
    fn declaration() -> Declaration; // "String"
    fn type_def() -> TypeDef; // Sequence { Char }
    fn definition() -> Definition {
        let type_def = Self::type_def();
        let uid = UidJsonValue(Self::UID);
        Definition { type_def, uid }
    }
}

impl<T> CasperABI for &T
where
    T: CasperABI,
{
    fn populate_definitions(definitions: &mut Definitions) {
        T::populate_definitions(definitions);
    }

    fn declaration() -> Declaration {
        T::declaration()
    }

    fn type_def() -> TypeDef {
        T::type_def()
    }
}

impl<T> CasperABI for Box<T>
where
    T: CasperABI,
{
    fn populate_definitions(definitions: &mut Definitions) {
        T::populate_definitions(definitions);
    }

    fn declaration() -> Declaration {
        T::declaration()
    }

    fn type_def() -> TypeDef {
        T::type_def()
    }
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

    (@impl $ty:ty => $def:expr ) => {
        impl CasperABI for $ty {
            fn populate_definitions(_definitions: &mut Definitions) {
            }

            fn declaration() -> Declaration {
                stringify!($def).into()
            }

            fn type_def() -> TypeDef {
                use Primitive::*;
                const PRIMITIVE: Primitive = $def;
                TypeDef::Primitive(PRIMITIVE)
            }
        }

        impl Keyable for $ty {
            const PRIMITIVE: Primitive = {
                use Primitive::*;
                $def
            };
        }
    };
}

impl CasperABI for () {
    fn populate_definitions(_definitions: &mut Definitions) {}

    fn declaration() -> Declaration {
        "()".into()
    }

    fn type_def() -> TypeDef {
        TypeDef::unit()
    }
}

impl_abi_for_types!(
    char => Char,
    bool => Bool,
    u8 => U8,
    u16 => U16,
    u32 => U32,
    u64 => U64,
    u128 => U128,
    i8 => I8,
    i16 => I16,
    i32 => I32,
    i64 => I64,
    f32 => F32,
    f64 => F64,
    i128 => I128,
);

#[impl_for_tuples(1, 12)]
impl CasperABI for Tuple {
    fn populate_definitions(_definitions: &mut Definitions) {
        for_tuples!( #( _definitions.populate_one::<Tuple>(); )* )
    }

    fn declaration() -> Declaration {
        let items = <[_]>::into_vec(Box::new([for_tuples!( #( Tuple::declaration() ),* )]));
        format!("({})", items.join(", "))
    }

    fn type_def() -> TypeDef {
        let items = <[_]>::into_vec(Box::new([for_tuples!( #( Tuple::declaration() ),* )]));
        TypeDef::Tuple { items }
    }
}

impl<T: CasperABI, E: CasperABI> CasperABI for Result<T, E> {
    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<T>();
        definitions.populate_one::<E>();
    }

    fn declaration() -> Declaration {
        let t_decl = T::declaration();
        let e_decl = E::declaration();
        format!("Result<{t_decl}, {e_decl}>")
    }

    fn type_def() -> TypeDef {
        TypeDef::Enum {
            items: vec![
                EnumVariant {
                    name: "Ok".into(),
                    discriminant: 0,
                    decl: T::declaration(),
                },
                EnumVariant {
                    name: "Err".into(),
                    discriminant: 1,
                    decl: E::declaration(),
                },
            ],
        }
    }
}

impl<T: CasperABI> CasperABI for Option<T> {
    fn declaration() -> Declaration {
        format!("Option<{}>", T::declaration())
    }
    fn type_def() -> TypeDef {
        TypeDef::Enum {
            items: vec![
                EnumVariant {
                    name: "None".into(),
                    discriminant: 0,
                    decl: <()>::declaration(),
                },
                EnumVariant {
                    name: "Some".into(),
                    discriminant: 1,
                    decl: T::declaration(),
                },
            ],
        }
    }

    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<()>();
        definitions.populate_one::<T>();
    }
}

impl<T: CasperABI> CasperABI for Vec<T> {
    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<T>();
    }

    fn declaration() -> Declaration {
        format!("Vec<{}>", T::declaration())
    }
    fn type_def() -> TypeDef {
        TypeDef::Sequence {
            decl: T::declaration(),
        }
    }
}

impl<T: CasperABI, const N: usize> CasperABI for [T; N] {
    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<T>();
    }

    fn declaration() -> Declaration {
        format!("[{}; {N}]", T::declaration())
    }
    fn type_def() -> TypeDef {
        TypeDef::FixedSequence {
            length: N.try_into().expect("N is too big"),
            decl: T::declaration(),
        }
    }
}

impl<K: CasperABI, V: CasperABI> CasperABI for BTreeMap<K, V> {
    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<K>();
        definitions.populate_one::<V>();
    }

    fn declaration() -> Declaration {
        format!("BTreeMap<{}, {}>", K::declaration(), V::declaration())
    }

    fn type_def() -> TypeDef {
        TypeDef::Mapping {
            key: K::declaration(),
            value: V::declaration(),
        }
    }
}

impl<K: CasperABI, V: CasperABI> CasperABI for HashMap<K, V> {
    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<K>();
        definitions.populate_one::<V>();
    }

    fn declaration() -> Declaration {
        format!("HashMap<{}, {}>", K::declaration(), V::declaration())
    }

    fn type_def() -> TypeDef {
        TypeDef::Mapping {
            key: K::declaration(),
            value: V::declaration(),
        }
    }
}

impl CasperABI for String {
    fn populate_definitions(_definitions: &mut Definitions) {}

    fn declaration() -> Declaration {
        "String".into()
    }
    fn type_def() -> TypeDef {
        TypeDef::Sequence {
            decl: char::declaration(),
        }
    }
}

impl CasperABI for str {
    fn populate_definitions(_definitions: &mut Definitions) {}

    fn declaration() -> Declaration {
        "String".into()
    }
    fn type_def() -> TypeDef {
        TypeDef::Sequence {
            decl: char::declaration(),
        }
    }
}

impl CasperABI for &str {
    fn populate_definitions(_definitions: &mut Definitions) {}

    fn declaration() -> Declaration {
        "String".into()
    }

    fn type_def() -> TypeDef {
        TypeDef::Sequence {
            decl: char::declaration(),
        }
    }
}

impl<T: CasperABI> CasperABI for LinkedList<T> {
    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<T>();
    }

    fn declaration() -> Declaration {
        format!("LinkedList<{}>", T::declaration())
    }
    fn type_def() -> TypeDef {
        TypeDef::Sequence {
            decl: T::declaration(),
        }
    }
}

impl<T: CasperABI> CasperABI for BTreeSet<T> {
    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<T>();
    }

    fn declaration() -> Declaration {
        format!("BTreeSet<{}>", T::declaration())
    }
    fn type_def() -> TypeDef {
        TypeDef::Sequence {
            decl: T::declaration(),
        }
    }
}

impl<const N: usize> CasperABI for bnum::BUint<N> {
    fn populate_definitions(definitions: &mut Definitions) {
        definitions.populate_one::<u64>();
    }

    fn declaration() -> Declaration {
        let width_bytes: usize = mem::size_of::<bnum::BUint<N>>();
        let width_bits: usize = width_bytes * 8;
        format!("U{width_bits}")
    }

    fn type_def() -> TypeDef {
        let length: u32 = N.try_into().expect("N is too big");
        TypeDef::FixedSequence {
            length,
            decl: u64::declaration(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        abi::{CasperABI, TypeDef},
        types::U256,
    };

    #[test]
    fn u256_schema() {
        assert_eq!(U256::declaration(), "U256");
        assert_eq!(
            U256::type_def(),
            TypeDef::FixedSequence {
                length: 4,
                decl: u64::declaration()
            }
        );

        let mut value = U256::from(u128::MAX);
        value += U256::from(1u64);
        let bytes = borsh::to_vec(&value).unwrap();
        // Ensure bnum's borsh serialize/deserialize is what we consider "FixedSequence"
        let bytes_back: [u64; 4] = borsh::from_slice(&bytes).unwrap();
        let value_back = U256::from_digits(bytes_back);
        assert_eq!(value, value_back);
    }
}
