use core::any::{Any, TypeId};

use crate::{
    compat::types::{CLType, CLTyped},
    prelude::{
        collections::{self, BTreeMap, BTreeSet, HashMap, LinkedList},
        str::FromStr,
    },
};
use casper_executor_wasm_common::type_uid::{self, TypeUid, Uid};
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
pub enum Definition {
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
    pub fn populate_one<T: CasperABI>(&mut self) {
        // T::populate_definitions(self);

        // let decl = T::declaration();
        // let def = T::definition();

        // self.populate_custom(decl, def);
        todo!()
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

/// Small builder that keeps up to 8 fragments on-stack before allocating.
pub type Declaration = String;

#[derive(Debug, Clone)]

pub struct ABITypeInfo {
    type_id: Uid,
    cl_type: CLType,
    declaration: Declaration,
    definition: Definition,
}

impl ABITypeInfo {
    pub fn new(
        type_id: Uid,
        cl_type: CLType,
        declaration: Declaration,
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

    pub fn declaration(&self) -> &Declaration {
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

    fn declaration() -> Declaration {
        std::any::type_name::<Self>().into()
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
        let items = <[_]>::into_vec(Box::new([for_tuples!( #( Tuple::declaration() ),* )]));
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
}

impl<T: CasperABI> CasperABI for Vec<T> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        T::visit(v);
    }

    fn definition() -> Definition {
        Definition::Sequence {
            decl: T::declaration(),
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
            decl: T::declaration(),
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
            key: K::declaration(),
            value: V::declaration(),
        }
    }
}

impl<K: CasperABI, V: CasperABI> CasperABI for HashMap<K, V> {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        K::visit(v);
        V::visit(v);
    }

    fn definition() -> Definition {
        Definition::Mapping {
            key: K::declaration(),
            value: V::declaration(),
        }
    }
}

impl CasperABI for String {
    fn definition() -> Definition {
        Definition::Sequence {
            decl: char::declaration(),
        }
    }
}

impl CasperABI for str {
    fn definition() -> Definition {
        Definition::Sequence {
            decl: char::declaration(),
        }
    }
}

impl CasperABI for &'static str {
    fn definition() -> Definition {
        Definition::Sequence {
            decl: char::declaration(),
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
            decl: T::declaration(),
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
            decl: T::declaration(),
        }
    }
}

impl CasperABI for crate::types::U256 {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        <[u64; 4]>::visit(v);
    }

    fn declaration() -> Declaration {
        "casper_contract_sdk::types::U256".into()
    }

    fn definition() -> Definition {
        Definition::FixedSequence {
            length: 4,
            decl: u64::declaration(),
        }
    }
}

impl CasperABI for crate::compat::types::U512 {
    fn visit(v: &mut dyn ABIVisitor) {
        v.accept(ABITypeInfo::from_abi_type::<Self>());
        <[u64; 8]>::visit(v);
    }

    fn declaration() -> Declaration {
        "casper_contract_sdk::compat::types::U512".into()
    }

    fn definition() -> Definition {
        Definition::FixedSequence {
            length: 8,
            decl: u64::declaration(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        abi::{
            visit_types_recursively, ABIVisitor, CasperABI, Declaration, Definition, EnumVariant,
            Primitive,
        },
        types::U256,
    };

    #[test]
    fn u256_schema() {
        assert_eq!(U256::declaration(), "U256");
        assert_eq!(
            U256::definition(),
            Definition::FixedSequence {
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

    #[test]
    fn visit_all_nested_types() {
        #[derive(Default)]
        struct Test {
            vec: Vec<(Declaration, Definition)>,
        }

        impl ABIVisitor for Test {
            fn accept(&mut self, declaration: Declaration, definition: Definition) {
                self.vec.push((declaration, definition));
            }
        }

        let mut test = Test::default();

        visit_types_recursively::<(Option<U256>, String)>(&mut test);
        assert_eq!(
            test.vec,
            vec![
                (
                    "(Option<U256>, String)".into(),
                    Definition::Tuple {
                        items: vec!["Option<U256>".into(), "String".into()]
                    }
                ),
                (
                    "Option<U256>".into(),
                    Definition::Enum {
                        items: vec![
                            EnumVariant {
                                name: "None".into(),
                                discriminant: 0,
                                decl: "()".into(),
                            },
                            EnumVariant {
                                name: "Some".into(),
                                discriminant: 1,
                                decl: "U256".into(),
                            },
                        ],
                    }
                ),
                (
                    "U256".into(),
                    Definition::FixedSequence {
                        length: 4,
                        decl: "U64".into(),
                    }
                ),
                ("U64".into(), Definition::Primitive(Primitive::U64)),
                (
                    "String".into(),
                    Definition::Sequence {
                        decl: "Char".into(),
                    }
                ),
            ]
        )

        // assert_eq!(test.vec.len(), 3);
    }
}
