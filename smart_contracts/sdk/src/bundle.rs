use crate::{
    prelude::collections::BTreeMap,
    schema::Schema,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

use bnum::cast::As;
use casper_executor_wasm_common::type_uid::Uid;
use serde::{Deserialize, Serialize};

use crate::{abi::Definition, compat::types::CLType};

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub enum BundlePrimitive {
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

impl From<crate::abi::Primitive> for BundlePrimitive {
    fn from(value: crate::abi::Primitive) -> Self {
        match value {
            crate::abi::Primitive::Char => BundlePrimitive::Char,
            crate::abi::Primitive::U8 => BundlePrimitive::U8,
            crate::abi::Primitive::I8 => BundlePrimitive::I8,
            crate::abi::Primitive::U16 => BundlePrimitive::U16,
            crate::abi::Primitive::I16 => BundlePrimitive::I16,
            crate::abi::Primitive::U32 => BundlePrimitive::U32,
            crate::abi::Primitive::I32 => BundlePrimitive::I32,
            crate::abi::Primitive::U64 => BundlePrimitive::U64,
            crate::abi::Primitive::I64 => BundlePrimitive::I64,
            crate::abi::Primitive::U128 => BundlePrimitive::U128,
            crate::abi::Primitive::I128 => BundlePrimitive::I128,
            crate::abi::Primitive::F32 => BundlePrimitive::F32,
            crate::abi::Primitive::F64 => BundlePrimitive::F64,
            crate::abi::Primitive::Bool => BundlePrimitive::Bool,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleEnumVariant {
    pub discriminant: u64,
    /// Optional declaration for the variant.
    ///
    /// Plain enum variants (i.e. those without any type, only discriminants) don't require a type
    /// declaration.
    pub decl: Option<Uid>,
}
#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleStructField {
    pub decl: Uid,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub enum BundleTypeDefinition {
    /// Primitive type.
    ///
    /// Examples: u64, i32, f32, bool, etc
    Primitive(BundlePrimitive),
    /// A mapping.
    ///
    /// Example Rust types: BTreeMap<K, V>.
    Mapping {
        key: Uid,
        value: Uid,
    },
    /// Arbitrary sequence of values.
    ///
    /// Example Rust types: `Vec<T>`, `&[T]`, `[T; N]`, `Box<[T]>`
    Sequence {
        /// If length is known, then it specifies that this definition should be be represented as
        /// an array of a fixed size.
        decl: Uid,
    },
    FixedSequence {
        /// If length is known, then it specifies that this definition should be be represented as
        /// an array of a fixed size.
        length: u32, // None -> Vec<T> Some(N) [T; N]
        decl: Uid,
    },
    /// A tuple of multiple values of various types.
    ///
    /// Can be also used to represent a heterogeneous list.
    Tuple {
        items: Vec<Uid>,
    },
    Enum {
        items: Vec<BundleEnumVariant>,
    },
    Struct {
        items: Vec<BundleStructField>,
    },
}

impl From<Definition> for BundleTypeDefinition {
    fn from(value: Definition) -> Self {
        match value {
            Definition::Primitive(p) => BundleTypeDefinition::Primitive(p.into()),
            Definition::Mapping { key, value } => BundleTypeDefinition::Mapping {
                key: key.as_uid(),
                value: value.as_uid(),
            },
            Definition::Sequence { decl } => BundleTypeDefinition::Sequence {
                decl: decl.as_uid(),
            },
            Definition::FixedSequence { length, decl } => BundleTypeDefinition::FixedSequence {
                length,
                decl: decl.as_uid(),
            },
            Definition::Tuple { items } => BundleTypeDefinition::Tuple {
                items: items
                    .into_iter()
                    .map(|schema_uid| schema_uid.as_uid())
                    .collect(),
            },
            Definition::Enum { items } => BundleTypeDefinition::Enum {
                items: items
                    .into_iter()
                    .map(|v| BundleEnumVariant {
                        discriminant: v.discriminant,
                        decl: v.decl.map(|schema_uid| schema_uid.as_uid()),
                    })
                    .collect(),
            },
            Definition::Struct { items } => BundleTypeDefinition::Struct {
                items: items
                    .into_iter()
                    .map(|f| BundleStructField {
                        decl: f.decl.as_uid(),
                    })
                    .collect(),
            },
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleDefinition {
    pub definition: BundleTypeDefinition,
    pub cl_type: CLType,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleArgument {
    pub decl: Uid,
}

bitflags::bitflags! {
    /// Flags for entry points.
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct BundleEntryPointFlags: u32 {
        /// The entry point is a constructor.
        const IS_CONSTRUCTOR = 1 << 0;
        /// The entry point is payable.
        const IS_PAYABLE = 1 << 1;
        /// The entry point is immutable. If this bit is not set, the entry point is mutable.
        const IS_IMMUTABLE = 1 << 2;
        /// Named ABI convention. If this bit is not set, the entry point uses positional ABI convention.
        const USES_NAMED_CONVENTION = 1 << 3;
    }
}

impl BorshSerialize for BundleEntryPointFlags {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        BorshSerialize::serialize(&self.bits(), writer)
    }
}

impl BorshDeserialize for BundleEntryPointFlags {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let bits = u32::deserialize_reader(reader)?;
        Ok(BundleEntryPointFlags::from_bits_truncate(bits))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleEntryPoint {
    pub name: String,
    pub export_name: String,
    pub arguments: Vec<BundleArgument>,
    pub result: Uid,
    pub flags: BundleEntryPointFlags,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleMessage {
    /// The topic of the message.
    ///
    /// This, unlike the type names etc, is crucial for discovering messages.
    pub topic: String,
    pub decl: Uid,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleV1 {
    definitions: BTreeMap<Uid, BundleDefinition>,
    entry_points: Vec<BundleEntryPoint>,
    messages: Vec<BundleMessage>,
}

impl BundleV1 {
    pub fn entry_points(&self) -> &[BundleEntryPoint] {
        &self.entry_points
    }

    pub fn messages(&self) -> &[BundleMessage] {
        &self.messages
    }

    pub fn definitions(&self) -> &BTreeMap<Uid, BundleDefinition> {
        &self.definitions
    }
}

impl From<Schema> for BundleV1 {
    fn from(schema: Schema) -> Self {
        let Schema {
            definitions,
            metadata: _,
            type_: _,
            declarations: _,
            entry_points,
            messages,
        } = schema;

        let bundle_definitions = definitions
            .0
            .into_iter()
            .map(|(k, v)| {
                (
                    k.as_uid(),
                    BundleDefinition {
                        definition: BundleTypeDefinition::from(v.definition),
                        cl_type: v.cl_type,
                    },
                )
            })
            .collect();

        let bundle_messages = messages
            .into_iter()
            .map(|msg| BundleMessage {
                topic: msg.topic,
                decl: msg.decl,
            })
            .collect::<Vec<_>>();

        let bundle_entry_points = entry_points
            .into_iter()
            .map(|ep| {
                let mut flags = BundleEntryPointFlags::empty();

                if ep.is_constructor {
                    flags |= BundleEntryPointFlags::IS_CONSTRUCTOR;
                }
                if ep.is_payable {
                    flags |= BundleEntryPointFlags::IS_PAYABLE;
                }
                match ep.receiver {
                    Some(crate::schema::SchemaReceiver::Immutable) => {
                        flags |= BundleEntryPointFlags::IS_IMMUTABLE;
                    }
                    Some(crate::schema::SchemaReceiver::Mutable) => {
                        // This is the default behavior
                    }
                    None => {
                        // Although the macro does not perform read/write state operations (there's
                        // no self that we can dispatch entry points onto),
                        // we consider entry points without a receiver as mutable to allow state
                        // modifications via runtime functions.
                    }
                }

                match ep.abi_convention {
                    crate::schema::SchemaAbiConvention::Named => {
                        flags |= BundleEntryPointFlags::USES_NAMED_CONVENTION;
                    }
                    crate::schema::SchemaAbiConvention::Positional => {
                        // Default behavior
                    }
                }

                BundleEntryPoint {
                    name: ep.name,
                    export_name: ep.export_name,
                    arguments: ep
                        .arguments
                        .into_iter()
                        .map(|arg| BundleArgument {
                            decl: Uid::from(arg.decl),
                        })
                        .collect(),
                    result: Uid::from(ep.result),
                    flags,
                }
            })
            .collect::<Vec<_>>();

        Self {
            definitions: bundle_definitions,
            entry_points: bundle_entry_points,
            messages: bundle_messages,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub enum Bundle {
    V1(BundleV1),
}

impl From<BundleV1> for Bundle {
    fn from(value: BundleV1) -> Self {
        Self::V1(value)
    }
}
