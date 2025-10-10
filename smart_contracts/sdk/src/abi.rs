pub mod collector;

use core::any::Any;
use crate::serializers::borsh::{BorshSerialize, BorshDeserialize};
#[cfg(feature = "std")]
use crate::prelude::collections::HashMap;
use crate::{
    compat::types::{CLType, CLTyped},
    prelude::{
        collections::{BTreeMap, BTreeSet, LinkedList},
        str::FromStr,
        Box, String, Vec,
    },
    schema::SchemaUid,
};
use casper_executor_wasm_common::type_uid::{self, TypeUid, Uid};
use impl_trait_for_tuples::impl_for_tuples;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, BorshSerialize, BorshDeserialize)]
pub struct EnumVariant {
    pub name: String,
    pub discriminant: u64,
    /// Optional declaration for the variant.
    ///
    /// Plain enum variants (i.e. those without any type, only discriminants) don't require a type
    /// declaration.
    pub decl: Option<SchemaUid>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, BorshSerialize, BorshDeserialize)]
pub struct StructField {
    pub name: String,
    pub decl: SchemaUid,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, BorshSerialize, BorshDeserialize)]
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Hash, BorshSerialize, BorshDeserialize)]
#[serde(tag = "type")]
pub enum Definition {
    /// Primitive type.
    ///
    /// Examples: u64, i32, f32, bool, etc
    Primitive(Primitive),
    /// A mapping.
    ///
    /// Example Rust types: BTreeMap<K, V>.
    Mapping {
        key: SchemaUid,
        value: SchemaUid,
    },
    /// Arbitrary sequence of values.
    ///
    /// Example Rust types: `Vec<T>`, `&[T]`, `[T; N]`, `Box<[T]>`
    Sequence {
        /// If length is known, then it specifies that this definition should be be represented as
        /// an array of a fixed size.
        decl: SchemaUid,
    },
    FixedSequence {
        /// If length is known, then it specifies that this definition should be be represented as
        /// an array of a fixed size.
        length: u32, // None -> Vec<T> Some(N) [T; N]
        decl: SchemaUid,
    },
    /// A tuple of multiple values of various types.
    ///
    /// Can be also used to represent a heterogeneous list.
    Tuple {
        items: Vec<SchemaUid>,
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

    pub fn as_tuple(&self) -> Option<&[SchemaUid]> {
        if let Self::Tuple { items } = self {
            Some(items.as_slice())
        } else {
            None
        }
    }
}

/// Small builder that keeps up to 8 fragments on-stack before allocating.
pub type AbiDeclaration = String;

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]

pub struct ABITypeInfo {
    type_id: Uid,
    cl_type: CLType,
    declaration: AbiDeclaration,
    definition: Definition,
}

impl ABITypeInfo {
    pub fn new(
        type_id: Uid,
        cl_type: CLType,
        declaration: AbiDeclaration,
        definition: Definition,
    ) -> Self {
        Self {
            type_id,
            cl_type,
            declaration,
            definition,
        }
    }

    pub fn from_abi_type<T: CasperABI + TypeUid>() -> Self
    where
        Self: Sized,
    {
        Self {
            type_id: type_uid::of::<T>(),
            cl_type: T::cl_type(),
            declaration: T::declaration(),
            definition: T::definition(),
        }
    }

    pub fn type_uid(&self) -> Uid {
        self.type_id
    }

    pub fn cl_type(&self) -> &CLType {
        &self.cl_type
    }

    pub fn declaration(&self) -> &AbiDeclaration {
        &self.declaration
    }

    pub fn definition(&self) -> &Definition {
        &self.definition
    }
}

// ...existing code...
pub trait ABIVisitor {
    fn accept(&mut self, type_info: ABITypeInfo);
}

pub trait CasperABI: Any + CLTyped + TypeUid {
    /// Visits all the nested generic types recursively.
    ///
    /// This should be empty implementation if a type does not have any generic types.
    ///
    /// Check out [`visit_types_recursively`] for more info.
    fn visit(visitor: &mut dyn ABIVisitor)
    where
        Self: Sized,
    {
        let type_info = ABITypeInfo::new(
            type_uid::of::<Self>(),
            Self::cl_type(),
            Self::declaration(),
            Self::definition(),
        );
        visitor.accept(type_info);
    }

    fn declaration() -> AbiDeclaration {
        core::any::type_name::<Self>().into()
    }

    fn definition() -> Definition; // Sequence { Char }
}

/// Visits all the nested generic types recursively.
pub fn visit_types_recursively<T: CasperABI>(visitor: &mut dyn ABIVisitor) {
    T::visit(visitor);
}

impl<T> CasperABI for &'static T
where
    T: CasperABI,
{
    fn definition() -> Definition {
        T::definition()
    }
}

impl<T> CasperABI for Box<T>
where
    T: CasperABI,
{
    fn definition() -> Definition {
        T::definition()
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


            fn definition() -> Definition {
                use Primitive::*;
                const PRIMITIVE: Primitive = $def;
                Definition::Primitive(PRIMITIVE)
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
    fn definition() -> Definition {
        Definition::unit()
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
    for_tuples!( where #( Tuple: CLTyped )* );

    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        for_tuples!( #( Tuple::visit(v); )* )
    }

    fn definition() -> Definition {
        // Precompute capacity for the tuple items using the for_tuples! repetition.
        let mut capacity = 0usize;
        for_tuples!( #( capacity += 1; )* );
        let mut items = Vec::with_capacity(capacity);
        for_tuples!( #( items.push(type_uid::of::<Tuple>().into()); )* );
        Definition::Tuple { items }
    }
}

impl<T: CasperABI, E: CasperABI> CasperABI for Result<T, E> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        T::visit(v);
        E::visit(v);
    }

    fn definition() -> Definition {
        Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "Ok".into(),
                    discriminant: 0,
                    decl: Some(type_uid::of::<T>().into()),
                },
                EnumVariant {
                    name: "Err".into(),
                    discriminant: 1,
                    decl: Some(type_uid::of::<E>().into()),
                },
            ],
        }
    }
}

impl<T: CasperABI> CasperABI for Option<T> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        T::visit(v);
    }

    fn definition() -> Definition {
        Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "None".into(),
                    discriminant: 0,
                    decl: None,
                },
                EnumVariant {
                    name: "Some".into(),
                    discriminant: 1,
                    decl: Some(type_uid::of::<T>().into()),
                },
            ],
        }
    }
}

impl<T: CasperABI> CasperABI for Vec<T> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        T::visit(v);
    }

    fn definition() -> Definition {
        Definition::Sequence {
            decl: type_uid::of::<T>().into(),
        }
    }
}

impl<T: CasperABI, const N: usize> CasperABI for [T; N] {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        T::visit(v);
    }

    fn definition() -> Definition {
        Definition::FixedSequence {
            length: N.try_into().expect("N is too big"),
            decl: type_uid::of::<T>().into(),
        }
    }
}

impl<K: CasperABI, V: CasperABI> CasperABI for BTreeMap<K, V> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        K::visit(v);
        V::visit(v);
    }

    fn definition() -> Definition {
        Definition::Mapping {
            key: type_uid::of::<K>().into(),
            value: type_uid::of::<V>().into(),
        }
    }
}

#[cfg(feature = "std")]
impl<K: CasperABI, V: CasperABI> CasperABI for HashMap<K, V> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        K::visit(v);
        V::visit(v);
    }

    fn definition() -> Definition {
        Definition::Mapping {
            key: type_uid::of::<K>().into(),
            value: type_uid::of::<V>().into(),
        }
    }
}

impl CasperABI for String {
    fn definition() -> Definition {
        Definition::Sequence {
            decl: type_uid::of::<char>().into(),
        }
    }
}

impl CasperABI for str {
    fn definition() -> Definition {
        Definition::Sequence {
            decl: type_uid::of::<char>().into(),
        }
    }
}

impl CasperABI for &'static str {
    fn definition() -> Definition {
        Definition::Sequence {
            decl: type_uid::of::<char>().into(),
        }
    }
}

impl<T: CasperABI + TypeUid> CasperABI for LinkedList<T> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        T::visit(v);
    }

    fn definition() -> Definition {
        Definition::Sequence {
            decl: type_uid::of::<T>().into(),
        }
    }
}

impl<T: CasperABI> CasperABI for BTreeSet<T> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        T::visit(v);
    }

    fn definition() -> Definition {
        Definition::Sequence {
            decl: type_uid::of::<T>().into(),
        }
    }
}

impl CasperABI for crate::types::U256 {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        <[u64; 4]>::visit(v);
    }

    fn declaration() -> AbiDeclaration {
        "casper_contract_sdk::types::U256".into()
    }

    fn definition() -> Definition {
        Definition::FixedSequence {
            length: 4,
            decl: type_uid::of::<u64>().into(),
        }
    }
}

impl CasperABI for crate::compat::types::U512 {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        <[u64; 8]>::visit(v);
    }

    fn declaration() -> AbiDeclaration {
        "casper_contract_sdk::compat::types::U512".into()
    }

    fn definition() -> Definition {
        Definition::FixedSequence {
            length: 8,
            decl: type_uid::of::<u64>().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        abi::{visit_types_recursively, ABITypeInfo, ABIVisitor, CasperABI, Definition},
        prelude::{collections, Vec},
        types::U256,
    };
    use casper_executor_wasm_common::type_uid::{self};

    #[test]
    fn u256_schema() {
        assert_eq!(U256::declaration(), "U256");
        assert_eq!(
            U256::definition(),
            Definition::FixedSequence {
                length: 4,
                decl: type_uid::of::<u64>().into(),
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

    #[test]
    fn visit_all_nested_types() {
        #[derive(Default, BorshSerialize, BorshDeserialize)]
        struct Test(Vec<ABITypeInfo>);

        // default derived
        impl ABIVisitor for Test {
            fn accept(&mut self, type_info: ABITypeInfo) {
                self.0.push(type_info);
            }
        }

        let mut test = Test::default();

        visit_types_recursively::<(Option<U256>, &str)>(&mut test);
        // Basic structural checks using UIDs
        let uids: collections::BTreeSet<_> = test.0.iter().map(|ti| ti.type_uid()).collect();
        assert!(uids.contains(&type_uid::of::<(Option<U256>, &str)>()));
        assert!(uids.contains(&type_uid::of::<Option<U256>>()));
        assert!(uids.contains(&type_uid::of::<U256>()));
        assert!(uids.contains(&type_uid::of::<u64>()));
        assert!(uids.contains(&type_uid::of::<&str>()));
        assert!(uids.contains(&type_uid::of::<char>()));

        // Spot-check definitions for U256 and String
        let def_of = |uid| {
            test.0
                .iter()
                .find(|ti| ti.type_uid() == uid)
                .unwrap()
                .definition()
                .clone()
        };
        assert_eq!(
            def_of(type_uid::of::<U256>()),
            Definition::FixedSequence {
                length: 4,
                decl: type_uid::of::<u64>().into()
            }
        );
        assert_eq!(
            def_of(type_uid::of::<&str>()),
            Definition::Sequence {
                decl: type_uid::of::<char>().into()
            }
        );

        // assert_eq!(test.vec.len(), 3);
    }
}
