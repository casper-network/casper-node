use crate::{
    prelude::collections::BTreeMap,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

use casper_executor_wasm_common::type_uid::Uid;

use crate::compat::types::CLType;

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub enum MetaPrimitive {
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

#[cfg(not(target_arch = "wasm32"))]
impl From<crate::abi::Primitive> for MetaPrimitive {
    fn from(value: crate::abi::Primitive) -> Self {
        match value {
            crate::abi::Primitive::Char => MetaPrimitive::Char,
            crate::abi::Primitive::U8 => MetaPrimitive::U8,
            crate::abi::Primitive::I8 => MetaPrimitive::I8,
            crate::abi::Primitive::U16 => MetaPrimitive::U16,
            crate::abi::Primitive::I16 => MetaPrimitive::I16,
            crate::abi::Primitive::U32 => MetaPrimitive::U32,
            crate::abi::Primitive::I32 => MetaPrimitive::I32,
            crate::abi::Primitive::U64 => MetaPrimitive::U64,
            crate::abi::Primitive::I64 => MetaPrimitive::I64,
            crate::abi::Primitive::U128 => MetaPrimitive::U128,
            crate::abi::Primitive::I128 => MetaPrimitive::I128,
            crate::abi::Primitive::F32 => MetaPrimitive::F32,
            crate::abi::Primitive::F64 => MetaPrimitive::F64,
            crate::abi::Primitive::Bool => MetaPrimitive::Bool,
        }
    }
}

// 1024

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct MetaEnumVariant {
    /// The name of the variant.
    pub name: String,
    pub discriminant: u64,
    /// Optional declaration for the variant.
    ///
    /// Plain enum variants (i.e. those without any type, only discriminants) don't require a type
    /// declaration.
    pub decl: Option<Uid>,
}
#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct MetaStructField {
    pub name: String,
    pub decl: Uid,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub enum MetaTypeDefinition {
    /// Primitive type.
    ///
    /// Examples: u64, i32, f32, bool, etc
    Primitive(MetaPrimitive),
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
        items: Vec<MetaEnumVariant>,
    },
    Struct {
        items: Vec<MetaStructField>,
    },
}

#[cfg(not(target_arch = "wasm32"))]
impl From<crate::abi::Definition> for MetaTypeDefinition {
    fn from(value: crate::abi::Definition) -> Self {
        use crate::abi::{EnumVariant, StructField};

        match value {
            crate::abi::Definition::Primitive(p) => MetaTypeDefinition::Primitive(p.into()),
            crate::abi::Definition::Mapping { key, value } => MetaTypeDefinition::Mapping {
                key: key.as_uid(),
                value: value.as_uid(),
            },
            crate::abi::Definition::Sequence { decl } => MetaTypeDefinition::Sequence {
                decl: decl.as_uid(),
            },
            crate::abi::Definition::FixedSequence { length, decl } => {
                MetaTypeDefinition::FixedSequence {
                    length,
                    decl: decl.as_uid(),
                }
            }
            crate::abi::Definition::Tuple { items } => MetaTypeDefinition::Tuple {
                items: items
                    .into_iter()
                    .map(|schema_uid| schema_uid.as_uid())
                    .collect(),
            },
            crate::abi::Definition::Enum { items } => MetaTypeDefinition::Enum {
                items: items
                    .into_iter()
                    .map(
                        |EnumVariant {
                             discriminant,
                             decl,
                             name,
                         }| MetaEnumVariant {
                            discriminant,
                            decl: decl.map(|schema_uid| schema_uid.as_uid()),
                            name,
                        },
                    )
                    .collect(),
            },
            crate::abi::Definition::Struct { items } => MetaTypeDefinition::Struct {
                items: items
                    .into_iter()
                    .map(|StructField { name, decl }| MetaStructField {
                        name,
                        decl: decl.as_uid(),
                    })
                    .collect(),
            },
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct MetaDefinition {
    pub uid: Uid,
    /// The name of the declaration (i.e. [`String`])
    pub name: String,
    /// The fully qualified name of the declaration (i.e. [`alloc::string::String`])
    pub fqn: String,
    /// The type definition.
    pub definition: MetaTypeDefinition,
    /// The [`CLType`] representation of this definition.
    pub cl_type: CLType,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct MetaArgument {
    pub name: String,
    pub decl: Uid,
}

bitflags::bitflags! {
    /// Flags for entry points.
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct MetaEntryPointFlags: u32 {
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

impl BorshSerialize for MetaEntryPointFlags {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        BorshSerialize::serialize(&self.bits(), writer)
    }
}

impl BorshDeserialize for MetaEntryPointFlags {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let bits = u32::deserialize_reader(reader)?;
        Ok(MetaEntryPointFlags::from_bits_truncate(bits))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct MetaEntryPoint {
    pub name: String,
    pub export_name: String,
    pub arguments: Vec<MetaArgument>,
    pub result: Uid,
    pub flags: MetaEntryPointFlags,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct MetaMessage {
    /// The topic of the message.
    ///
    /// This, unlike the type names etc, is crucial for discovering messages.
    pub topic: String,
    pub decl: Uid,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct MetaV1 {
    wasm_hash: [u8; 32],
    definitions: Vec<MetaDefinition>,
    entry_points: Vec<MetaEntryPoint>,
    messages: Vec<MetaMessage>,
    metadata: BTreeMap<String, Vec<String>>,
}

impl MetaV1 {
    pub fn new(
        wasm_hash: [u8; 32],
        definitions: Vec<MetaDefinition>,
        entry_points: Vec<MetaEntryPoint>,
        messages: Vec<MetaMessage>,
        metadata: BTreeMap<String, Vec<String>>,
    ) -> Self {
        Self {
            wasm_hash,
            definitions,
            entry_points,
            messages,
            metadata,
        }
    }

    pub fn wasm_hash(&self) -> &[u8; 32] {
        &self.wasm_hash
    }

    pub fn entry_points(&self) -> &[MetaEntryPoint] {
        &self.entry_points
    }

    pub fn messages(&self) -> &[MetaMessage] {
        &self.messages
    }

    pub fn definitions(&self) -> &Vec<MetaDefinition> {
        &self.definitions
    }

    pub fn metadata(&self) -> &BTreeMap<String, Vec<String>> {
        &self.metadata
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Meta {
    pub fn from_schema(schema: crate::schema::Schema, wasm_hash: [u8; 32]) -> Result<Meta, String> {
        let crate::schema::Schema {
            metadata,
            type_: _,
            declarations,
            definitions,
            entry_points,
            messages,
        } = schema;

        let mut meta_definitions = Vec::new();

        for (schema_uid, _schema_def) in definitions.0 {
            let _decl = declarations.0.get(&schema_uid).ok_or_else(|| {
                format!(
                    "missing declaration for definition UID {}",
                    schema_uid.as_uid()
                )
            })?;

            let _meta_def = MetaDefinition {
                uid: schema_uid.as_uid(),
                name: _decl.name.clone(),
                fqn: _decl.name.clone(),
                definition: {
                    use crate::abi::{Definition, EnumVariant, StructField};

                    match _schema_def.definition {
                        Definition::Primitive(p) => MetaTypeDefinition::Primitive(p.into()),
                        Definition::Mapping { key, value } => MetaTypeDefinition::Mapping {
                            key: key.as_uid(),
                            value: value.as_uid(),
                        },
                        Definition::Sequence { decl } => MetaTypeDefinition::Sequence {
                            decl: decl.as_uid(),
                        },
                        Definition::FixedSequence { length, decl } => {
                            MetaTypeDefinition::FixedSequence {
                                length,
                                decl: decl.as_uid(),
                            }
                        }
                        Definition::Tuple { items } => MetaTypeDefinition::Tuple {
                            items: items
                                .into_iter()
                                .map(|schema_uid| schema_uid.as_uid())
                                .collect(),
                        },
                        Definition::Enum { items } => MetaTypeDefinition::Enum {
                            items: items
                                .into_iter()
                                .map(
                                    |EnumVariant {
                                         discriminant,
                                         decl,
                                         name,
                                     }| MetaEnumVariant {
                                        discriminant,
                                        decl: decl.map(|schema_uid| schema_uid.as_uid()),
                                        name,
                                    },
                                )
                                .collect(),
                        },
                        Definition::Struct { items } => MetaTypeDefinition::Struct {
                            items: items
                                .into_iter()
                                .map(|StructField { name, decl }| MetaStructField {
                                    decl: decl.as_uid(),
                                    name,
                                })
                                .collect(),
                        },
                    }
                },

                cl_type: _schema_def.cl_type,
            };

            meta_definitions.push(_meta_def);
        }

        let meta_messages = messages
            .into_iter()
            .map(|msg| MetaMessage {
                topic: msg.topic,
                decl: msg.decl,
            })
            .collect::<Vec<_>>();

        let meta_entry_points = entry_points
            .into_iter()
            .map(|ep| {
                let mut flags = MetaEntryPointFlags::empty();

                if ep.is_constructor {
                    flags |= MetaEntryPointFlags::IS_CONSTRUCTOR;
                }
                if ep.is_payable {
                    flags |= MetaEntryPointFlags::IS_PAYABLE;
                }
                match ep.receiver {
                    Some(crate::schema::SchemaReceiver::Immutable) => {
                        flags |= MetaEntryPointFlags::IS_IMMUTABLE;
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
                        flags |= MetaEntryPointFlags::USES_NAMED_CONVENTION;
                    }
                    crate::schema::SchemaAbiConvention::Positional => {
                        // Default behavior
                    }
                }

                MetaEntryPoint {
                    name: ep.name,
                    export_name: ep.export_name,
                    arguments: ep
                        .arguments
                        .into_iter()
                        .map(|arg| MetaArgument {
                            name: arg.name,
                            decl: arg.decl,
                        })
                        .collect(),
                    result: ep.result,
                    flags,
                }
            })
            .collect::<Vec<_>>();

        let crate::schema::SchemaMetadata {
            name,
            version,
            authors,
            description,
            rust_version,
        } = metadata;

        let mut meta = BTreeMap::new();

        if let Some(name) = name {
            meta.insert("name".to_string(), vec![name]);
        }
        if let Some(version) = version {
            meta.insert("version".to_string(), vec![version]);
        }
        if let Some(authors) = authors {
            meta.insert("authors".to_string(), authors);
        }
        if let Some(description) = description {
            meta.insert("description".to_string(), vec![description]);
        }
        if let Some(rust_version) = rust_version {
            meta.insert("rust_version".to_string(), vec![rust_version]);
        }

        Ok(Meta::V1(MetaV1 {
            wasm_hash,
            definitions: meta_definitions,
            entry_points: meta_entry_points,
            messages: meta_messages,
            metadata: meta,
        }))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub enum Meta {
    V1(MetaV1),
}

impl From<MetaV1> for Meta {
    fn from(value: MetaV1) -> Self {
        Self::V1(value)
    }
}
