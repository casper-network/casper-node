use alloc::{string::String, vec::Vec};
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{
        self, Error, FromBytes, ToBytes, U32_SERIALIZED_LENGTH, U64_SERIALIZED_LENGTH,
        U8_SERIALIZED_LENGTH,
    },
    CLType,
};

/// Unique identifier for a type definition.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[repr(transparent)]
pub struct TypeUid(u32);

impl TypeUid {
    /// A `TypeUid` representing an untyped value.
    /// This is used for values that do not have a specific type, such as `()`.
    pub const UNTYPED: Self = TypeUid(0);

    /// Creates a new [`TypeUid`] from a raw `u32` value.
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw `u32` value of this UID.
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl Display for TypeUid {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:08x}", self.0)
    }
}

impl From<u32> for TypeUid {
    fn from(value: u32) -> Self {
        TypeUid(value)
    }
}

impl From<TypeUid> for u32 {
    fn from(value: TypeUid) -> Self {
        value.value()
    }
}

impl ToBytes for TypeUid {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        U32_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.0.write_bytes(writer)
    }
}

impl FromBytes for TypeUid {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (raw, rem) = u32::from_bytes(bytes)?;
        Ok((TypeUid::new(raw), rem))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct TypeDefinition {
    /// Uid
    pub uid: TypeUid,
    /// Name of the type definition (i.e. [`String`]).
    pub name: String,
    /// The fully qualified name of the declaration (i.e. [`alloc::string::String`]).
    pub fqn: String,
    pub definition: TypeDefinitionKind,
    pub cl_type: CLType,
}

impl TypeDefinition {
    /// Creates a new [`TypeDefinition`].
    pub fn new(
        uid: TypeUid,
        name: String,
        fqn: String,
        definition: TypeDefinitionKind,
        cl_type: CLType,
    ) -> Self {
        Self {
            uid,
            name,
            fqn,
            definition,
            cl_type,
        }
    }
}

impl ToBytes for TypeDefinition {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            uid,
            name,
            fqn,
            definition,
            cl_type,
        } = self;
        uid.serialized_length()
            + name.serialized_length()
            + fqn.serialized_length()
            + definition.serialized_length()
            + cl_type.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        let Self {
            uid,
            name,
            fqn,
            definition,
            cl_type,
        } = self;
        uid.write_bytes(writer)?;
        name.write_bytes(writer)?;
        fqn.write_bytes(writer)?;
        definition.write_bytes(writer)?;
        cl_type.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for TypeDefinition {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (uid, rem) = FromBytes::from_bytes(bytes)?;
        let (name, rem) = FromBytes::from_bytes(rem)?;
        let (fqn, rem) = FromBytes::from_bytes(rem)?;
        let (definition, rem) = FromBytes::from_bytes(rem)?;
        let (cl_type, rem) = FromBytes::from_bytes(rem)?;

        Ok((
            TypeDefinition {
                uid,
                name,
                fqn,
                definition,
                cl_type,
            },
            rem,
        ))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct TypeMessage {
    pub topic: String,
    pub decl: TypeUid,
}

impl ToBytes for TypeMessage {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.topic.serialized_length() + self.decl.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.topic.write_bytes(writer)?;
        self.decl.write_bytes(writer)
    }
}

impl FromBytes for TypeMessage {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (topic, rem) = String::from_bytes(bytes)?;
        let (decl, rem) = TypeUid::from_bytes(rem)?;
        Ok((TypeMessage { topic, decl }, rem))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct EnumVariant {
    pub name: String,
    pub discriminant: u64,
    pub decl: Option<TypeUid>,
}

impl ToBytes for EnumVariant {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length() + U64_SERIALIZED_LENGTH + self.decl.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.name.write_bytes(writer)?;
        self.discriminant.write_bytes(writer)?;
        self.decl.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for EnumVariant {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (name, rem) = String::from_bytes(bytes)?;
        let (discriminant, rem) = u64::from_bytes(rem)?;
        let (decl, rem) = Option::<TypeUid>::from_bytes(rem)?;
        Ok((
            EnumVariant {
                name,
                discriminant,
                decl,
            },
            rem,
        ))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct StructField {
    pub name: String,
    pub decl: TypeUid,
}

impl ToBytes for StructField {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.name.serialized_length() + self.decl.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.name.write_bytes(writer)?;
        self.decl.write_bytes(writer)
    }
}

impl FromBytes for StructField {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (name, rem) = String::from_bytes(bytes)?;
        let (decl, rem) = TypeUid::from_bytes(rem)?;
        Ok((StructField { name, decl }, rem))
    }
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PrimitiveTag {
    Char = 0,
    U8 = 1,
    I8 = 2,
    U16 = 3,
    I16 = 4,
    U32 = 5,
    I32 = 6,
    U64 = 7,
    I64 = 8,
    U128 = 9,
    I128 = 10,
    F32 = 11,
    F64 = 12,
    Bool = 13,
}

impl TryFrom<u8> for PrimitiveTag {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(PrimitiveTag::Char),
            1 => Ok(PrimitiveTag::U8),
            2 => Ok(PrimitiveTag::I8),
            3 => Ok(PrimitiveTag::U16),
            4 => Ok(PrimitiveTag::I16),
            5 => Ok(PrimitiveTag::U32),
            6 => Ok(PrimitiveTag::I32),
            7 => Ok(PrimitiveTag::U64),
            8 => Ok(PrimitiveTag::I64),
            9 => Ok(PrimitiveTag::U128),
            10 => Ok(PrimitiveTag::I128),
            11 => Ok(PrimitiveTag::F32),
            12 => Ok(PrimitiveTag::F64),
            13 => Ok(PrimitiveTag::Bool),
            _ => Err(Error::Formatting),
        }
    }
}

impl From<Primitive> for PrimitiveTag {
    fn from(value: Primitive) -> Self {
        match value {
            Primitive::Char => PrimitiveTag::Char,
            Primitive::U8 => PrimitiveTag::U8,
            Primitive::I8 => PrimitiveTag::I8,
            Primitive::U16 => PrimitiveTag::U16,
            Primitive::I16 => PrimitiveTag::I16,
            Primitive::U32 => PrimitiveTag::U32,
            Primitive::I32 => PrimitiveTag::I32,
            Primitive::U64 => PrimitiveTag::U64,
            Primitive::I64 => PrimitiveTag::I64,
            Primitive::U128 => PrimitiveTag::U128,
            Primitive::I128 => PrimitiveTag::I128,
            Primitive::F32 => PrimitiveTag::F32,
            Primitive::F64 => PrimitiveTag::F64,
            Primitive::Bool => PrimitiveTag::Bool,
        }
    }
}

impl From<PrimitiveTag> for Primitive {
    fn from(tag: PrimitiveTag) -> Self {
        match tag {
            PrimitiveTag::Char => Primitive::Char,
            PrimitiveTag::U8 => Primitive::U8,
            PrimitiveTag::I8 => Primitive::I8,
            PrimitiveTag::U16 => Primitive::U16,
            PrimitiveTag::I16 => Primitive::I16,
            PrimitiveTag::U32 => Primitive::U32,
            PrimitiveTag::I32 => Primitive::I32,
            PrimitiveTag::U64 => Primitive::U64,
            PrimitiveTag::I64 => Primitive::I64,
            PrimitiveTag::U128 => Primitive::U128,
            PrimitiveTag::I128 => Primitive::I128,
            PrimitiveTag::F32 => Primitive::F32,
            PrimitiveTag::F64 => Primitive::F64,
            PrimitiveTag::Bool => Primitive::Bool,
        }
    }
}

impl From<PrimitiveTag> for u8 {
    fn from(tag: PrimitiveTag) -> Self {
        tag as u8
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
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

impl Display for Primitive {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Primitive::Char => write!(f, "Char"),
            Primitive::U8 => write!(f, "U8"),
            Primitive::I8 => write!(f, "I8"),
            Primitive::U16 => write!(f, "U16"),
            Primitive::I16 => write!(f, "I16"),
            Primitive::U32 => write!(f, "U32"),
            Primitive::I32 => write!(f, "I32"),
            Primitive::U64 => write!(f, "U64"),
            Primitive::I64 => write!(f, "I64"),
            Primitive::U128 => write!(f, "U128"),
            Primitive::I128 => write!(f, "I128"),
            Primitive::F32 => write!(f, "F32"),
            Primitive::F64 => write!(f, "F64"),
            Primitive::Bool => write!(f, "Bool"),
        }
    }
}

impl ToBytes for Primitive {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        u8::from(PrimitiveTag::from(*self)).to_bytes()
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        u8::from(PrimitiveTag::from(*self)).write_bytes(writer)
    }
}

impl FromBytes for Primitive {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        let primitive = PrimitiveTag::try_from(tag)?.into();
        Ok((primitive, rem))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum TypeDefinitionKind {
    Primitive(Primitive),
    Mapping { key: TypeUid, value: TypeUid },
    Sequence { decl: TypeUid },
    FixedSequence { length: u32, decl: TypeUid },
    Tuple { items: Vec<TypeUid> },
    Enum { items: Vec<EnumVariant> },
    Struct { items: Vec<StructField> },
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DefinitionTag {
    Primitive = 0,
    Mapping = 1,
    Sequence = 2,
    FixedSequence = 3,
    Tuple = 4,
    Enum = 5,
    Struct = 6,
}

impl TryFrom<u8> for DefinitionTag {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DefinitionTag::Primitive),
            1 => Ok(DefinitionTag::Mapping),
            2 => Ok(DefinitionTag::Sequence),
            3 => Ok(DefinitionTag::FixedSequence),
            4 => Ok(DefinitionTag::Tuple),
            5 => Ok(DefinitionTag::Enum),
            6 => Ok(DefinitionTag::Struct),
            _ => Err(Error::Formatting),
        }
    }
}

impl From<TypeDefinitionKind> for DefinitionTag {
    fn from(value: TypeDefinitionKind) -> Self {
        match value {
            TypeDefinitionKind::Primitive(..) => DefinitionTag::Primitive,
            TypeDefinitionKind::Mapping { .. } => DefinitionTag::Mapping,
            TypeDefinitionKind::Sequence { .. } => DefinitionTag::Sequence,
            TypeDefinitionKind::FixedSequence { .. } => DefinitionTag::FixedSequence,
            TypeDefinitionKind::Tuple { .. } => DefinitionTag::Tuple,
            TypeDefinitionKind::Enum { .. } => DefinitionTag::Enum,
            TypeDefinitionKind::Struct { .. } => DefinitionTag::Struct,
        }
    }
}

impl From<&TypeDefinitionKind> for DefinitionTag {
    fn from(value: &TypeDefinitionKind) -> Self {
        match value {
            TypeDefinitionKind::Primitive(..) => DefinitionTag::Primitive,
            TypeDefinitionKind::Mapping { .. } => DefinitionTag::Mapping,
            TypeDefinitionKind::Sequence { .. } => DefinitionTag::Sequence,
            TypeDefinitionKind::FixedSequence { .. } => DefinitionTag::FixedSequence,
            TypeDefinitionKind::Tuple { .. } => DefinitionTag::Tuple,
            TypeDefinitionKind::Enum { .. } => DefinitionTag::Enum,
            TypeDefinitionKind::Struct { .. } => DefinitionTag::Struct,
        }
    }
}

impl From<DefinitionTag> for u8 {
    fn from(tag: DefinitionTag) -> Self {
        tag as u8
    }
}

impl TypeDefinitionKind {
    fn tag(&self) -> DefinitionTag {
        DefinitionTag::from(self)
    }

    fn content_serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TypeDefinitionKind::Primitive(primitive) => primitive.serialized_length(),
                TypeDefinitionKind::Mapping { key, value } => {
                    key.serialized_length() + value.serialized_length()
                }
                TypeDefinitionKind::Sequence { decl } => decl.serialized_length(),
                TypeDefinitionKind::FixedSequence { length: _, decl } => {
                    U32_SERIALIZED_LENGTH + decl.serialized_length()
                }
                TypeDefinitionKind::Tuple { items } => items.serialized_length(),
                TypeDefinitionKind::Enum { items } => items.serialized_length(),
                TypeDefinitionKind::Struct { items } => items.serialized_length(),
            }
    }
}

impl ToBytes for TypeDefinitionKind {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.content_serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.push(u8::from(self.tag()));
        match self {
            TypeDefinitionKind::Primitive(primitive) => primitive.write_bytes(writer),
            TypeDefinitionKind::Mapping { key, value } => {
                key.write_bytes(writer)?;
                value.write_bytes(writer)
            }
            TypeDefinitionKind::Sequence { decl } => decl.write_bytes(writer),
            TypeDefinitionKind::FixedSequence { length, decl } => {
                length.write_bytes(writer)?;
                decl.write_bytes(writer)
            }
            TypeDefinitionKind::Tuple { items } => items.write_bytes(writer),
            TypeDefinitionKind::Enum { items } => items.write_bytes(writer),
            TypeDefinitionKind::Struct { items } => items.write_bytes(writer),
        }
    }
}

impl FromBytes for TypeDefinitionKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (raw_tag, rem) = u8::from_bytes(bytes)?;
        match DefinitionTag::try_from(raw_tag)? {
            DefinitionTag::Primitive => {
                let (primitive, rem) = Primitive::from_bytes(rem)?;
                Ok((TypeDefinitionKind::Primitive(primitive), rem))
            }
            DefinitionTag::Mapping => {
                let (key, rem) = TypeUid::from_bytes(rem)?;
                let (value, rem) = TypeUid::from_bytes(rem)?;
                Ok((TypeDefinitionKind::Mapping { key, value }, rem))
            }
            DefinitionTag::Sequence => {
                let (decl, rem) = TypeUid::from_bytes(rem)?;
                Ok((TypeDefinitionKind::Sequence { decl }, rem))
            }
            DefinitionTag::FixedSequence => {
                let (length, rem) = u32::from_bytes(rem)?;
                let (decl, rem) = TypeUid::from_bytes(rem)?;
                Ok((TypeDefinitionKind::FixedSequence { length, decl }, rem))
            }
            DefinitionTag::Tuple => {
                let (items, rem) = Vec::<TypeUid>::from_bytes(rem)?;
                Ok((TypeDefinitionKind::Tuple { items }, rem))
            }
            DefinitionTag::Enum => {
                let (items, rem) = Vec::<EnumVariant>::from_bytes(rem)?;
                Ok((TypeDefinitionKind::Enum { items }, rem))
            }
            DefinitionTag::Struct => {
                let (items, rem) = Vec::<StructField>::from_bytes(rem)?;
                Ok((TypeDefinitionKind::Struct { items }, rem))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytesrepr::test_serialization_roundtrip;

    #[test]
    fn primitive_roundtrip() {
        let p = Primitive::F64;
        test_serialization_roundtrip(&p);
    }

    #[test]
    fn enum_variant_and_struct_field_roundtrip() {
        let variant = EnumVariant {
            name: "VariantA".to_string(),
            discriminant: 42,
            decl: Some(TypeUid::new(7)),
        };
        test_serialization_roundtrip(&variant);

        let field = StructField {
            name: "field1".to_string(),
            decl: TypeUid::new(8),
        };
        test_serialization_roundtrip(&field);
    }

    #[test]
    fn definition_variants_roundtrip() {
        // Primitive
        let def_prim = TypeDefinitionKind::Primitive(Primitive::Bool);
        test_serialization_roundtrip(&def_prim);

        // Mapping
        let def_map = TypeDefinitionKind::Mapping {
            key: TypeUid::new(1),
            value: TypeUid::new(2),
        };
        test_serialization_roundtrip(&def_map);

        // Sequence
        let def_seq = TypeDefinitionKind::Sequence {
            decl: TypeUid::new(3),
        };
        test_serialization_roundtrip(&def_seq);

        // FixedSequence
        let def_fixed = TypeDefinitionKind::FixedSequence {
            length: 10,
            decl: TypeUid::new(4),
        };
        test_serialization_roundtrip(&def_fixed);

        // Tuple
        let def_tuple = TypeDefinitionKind::Tuple {
            items: vec![TypeUid::new(5), TypeUid::new(6)],
        };
        test_serialization_roundtrip(&def_tuple);

        // Enum
        let enum_items = vec![
            EnumVariant {
                name: "Variant0".to_string(),
                discriminant: 0,
                decl: None,
            },
            EnumVariant {
                name: "Variant1".to_string(),
                discriminant: 1,
                decl: Some(TypeUid::new(9)),
            },
        ];
        let def_enum = TypeDefinitionKind::Enum { items: enum_items };
        test_serialization_roundtrip(&def_enum);

        // Struct
        let struct_items = vec![
            StructField {
                name: "field1".to_string(),

                decl: TypeUid::new(10),
            },
            StructField {
                name: "field2".to_string(),
                decl: TypeUid::new(11),
            },
        ];
        let def_struct = TypeDefinitionKind::Struct {
            items: struct_items,
        };
        test_serialization_roundtrip(&def_struct);
    }
}
