use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

use bitflags::bitflags;
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
    pub definition: Definition,
    pub cl_type: CLType,
}

impl TypeDefinition {
    pub fn new(definition: Definition, cl_type: CLType) -> Self {
        Self {
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
        self.definition.serialized_length() + self.cl_type.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.definition.write_bytes(writer)?;
        self.cl_type.write_bytes(writer)
    }
}

impl FromBytes for TypeDefinition {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (definition, rem) = Definition::from_bytes(bytes)?;
        let (cl_type, rem) = CLType::from_bytes(rem)?;
        Ok((
            TypeDefinition {
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
        U64_SERIALIZED_LENGTH + self.decl.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.discriminant.write_bytes(writer)?;
        self.decl.write_bytes(writer)
    }
}

impl FromBytes for EnumVariant {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (discriminant, rem) = u64::from_bytes(bytes)?;
        let (decl, rem) = Option::<TypeUid>::from_bytes(rem)?;
        Ok((EnumVariant { discriminant, decl }, rem))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct StructField {
    pub decl: TypeUid,
}

impl ToBytes for StructField {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.decl.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.decl.write_bytes(writer)
    }
}

impl FromBytes for StructField {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (decl, rem) = TypeUid::from_bytes(bytes)?;
        Ok((StructField { decl }, rem))
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

impl ToString for Primitive {
    fn to_string(&self) -> String {
        match self {
            Primitive::Char => "Char",
            Primitive::U8 => "U8",
            Primitive::I8 => "I8",
            Primitive::U16 => "U16",
            Primitive::I16 => "I16",
            Primitive::U32 => "U32",
            Primitive::I32 => "I32",
            Primitive::U64 => "U64",
            Primitive::I64 => "I64",
            Primitive::U128 => "U128",
            Primitive::I128 => "I128",
            Primitive::F32 => "F32",
            Primitive::F64 => "F64",
            Primitive::Bool => "Bool",
        }
        .to_string()
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
pub enum Definition {
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

impl From<Definition> for DefinitionTag {
    fn from(value: Definition) -> Self {
        match value {
            Definition::Primitive(..) => DefinitionTag::Primitive,
            Definition::Mapping { .. } => DefinitionTag::Mapping,
            Definition::Sequence { .. } => DefinitionTag::Sequence,
            Definition::FixedSequence { .. } => DefinitionTag::FixedSequence,
            Definition::Tuple { .. } => DefinitionTag::Tuple,
            Definition::Enum { .. } => DefinitionTag::Enum,
            Definition::Struct { .. } => DefinitionTag::Struct,
        }
    }
}

impl From<&Definition> for DefinitionTag {
    fn from(value: &Definition) -> Self {
        match value {
            Definition::Primitive(..) => DefinitionTag::Primitive,
            Definition::Mapping { .. } => DefinitionTag::Mapping,
            Definition::Sequence { .. } => DefinitionTag::Sequence,
            Definition::FixedSequence { .. } => DefinitionTag::FixedSequence,
            Definition::Tuple { .. } => DefinitionTag::Tuple,
            Definition::Enum { .. } => DefinitionTag::Enum,
            Definition::Struct { .. } => DefinitionTag::Struct,
        }
    }
}

impl From<DefinitionTag> for u8 {
    fn from(tag: DefinitionTag) -> Self {
        tag as u8
    }
}

impl Definition {
    fn tag(&self) -> DefinitionTag {
        DefinitionTag::from(self)
    }

    fn content_serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Definition::Primitive(primitive) => primitive.serialized_length(),
                Definition::Mapping { key, value } => {
                    key.serialized_length() + value.serialized_length()
                }
                Definition::Sequence { decl } => decl.serialized_length(),
                Definition::FixedSequence { length: _, decl } => {
                    U32_SERIALIZED_LENGTH + decl.serialized_length()
                }
                Definition::Tuple { items } => items.serialized_length(),
                Definition::Enum { items } => items.serialized_length(),
                Definition::Struct { items } => items.serialized_length(),
            }
    }
}

impl ToBytes for Definition {
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
            Definition::Primitive(primitive) => primitive.write_bytes(writer),
            Definition::Mapping { key, value } => {
                key.write_bytes(writer)?;
                value.write_bytes(writer)
            }
            Definition::Sequence { decl } => decl.write_bytes(writer),
            Definition::FixedSequence { length, decl } => {
                length.write_bytes(writer)?;
                decl.write_bytes(writer)
            }
            Definition::Tuple { items } => items.write_bytes(writer),
            Definition::Enum { items } => items.write_bytes(writer),
            Definition::Struct { items } => items.write_bytes(writer),
        }
    }
}

impl FromBytes for Definition {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (raw_tag, rem) = u8::from_bytes(bytes)?;
        match DefinitionTag::try_from(raw_tag)? {
            DefinitionTag::Primitive => {
                let (primitive, rem) = Primitive::from_bytes(rem)?;
                Ok((Definition::Primitive(primitive), rem))
            }
            DefinitionTag::Mapping => {
                let (key, rem) = TypeUid::from_bytes(rem)?;
                let (value, rem) = TypeUid::from_bytes(rem)?;
                Ok((Definition::Mapping { key, value }, rem))
            }
            DefinitionTag::Sequence => {
                let (decl, rem) = TypeUid::from_bytes(rem)?;
                Ok((Definition::Sequence { decl }, rem))
            }
            DefinitionTag::FixedSequence => {
                let (length, rem) = u32::from_bytes(rem)?;
                let (decl, rem) = TypeUid::from_bytes(rem)?;
                Ok((Definition::FixedSequence { length, decl }, rem))
            }
            DefinitionTag::Tuple => {
                let (items, rem) = Vec::<TypeUid>::from_bytes(rem)?;
                Ok((Definition::Tuple { items }, rem))
            }
            DefinitionTag::Enum => {
                let (items, rem) = Vec::<EnumVariant>::from_bytes(rem)?;
                Ok((Definition::Enum { items }, rem))
            }
            DefinitionTag::Struct => {
                let (items, rem) = Vec::<StructField>::from_bytes(rem)?;
                Ok((Definition::Struct { items }, rem))
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
    fn type_uid_and_argument_roundtrip() {
        let uid = TypeUid::new(0xdeadbeef);
        test_serialization_roundtrip(&uid);

        let arg = TypeArgument { decl: uid };
        test_serialization_roundtrip(&arg);
    }

    #[test]
    fn enum_variant_and_struct_field_roundtrip() {
        let variant = EnumVariant {
            name: String::from("VariantA"),
            discriminant: 42,
            decl: Some(TypeUid::new(7)),
        };
        test_serialization_roundtrip(&variant);

        let field = StructField {
            name: String::from("field1"),
            decl: TypeUid::new(8),
        };
        test_serialization_roundtrip(&field);
    }

    #[test]
    fn definition_variants_roundtrip() {
        // Primitive
        let def_prim = Definition::Primitive(Primitive::Bool);
        test_serialization_roundtrip(&def_prim);

        // Mapping
        let def_map = Definition::Mapping {
            key: TypeUid::new(1),
            value: TypeUid::new(2),
        };
        test_serialization_roundtrip(&def_map);

        // Sequence
        let def_seq = Definition::Sequence {
            decl: TypeUid::new(3),
        };
        test_serialization_roundtrip(&def_seq);

        // FixedSequence
        let def_fixed = Definition::FixedSequence {
            length: 10,
            decl: TypeUid::new(4),
        };
        test_serialization_roundtrip(&def_fixed);

        // Tuple
        let def_tuple = Definition::Tuple {
            items: vec![TypeUid::new(5), TypeUid::new(6)],
        };
        test_serialization_roundtrip(&def_tuple);

        // Enum
        let enum_items = vec![
            EnumVariant {
                name: String::from("A"),
                discriminant: 0,
                decl: None,
            },
            EnumVariant {
                name: String::from("B"),
                discriminant: 1,
                decl: Some(TypeUid::new(9)),
            },
        ];
        let def_enum = Definition::Enum { items: enum_items };
        test_serialization_roundtrip(&def_enum);

        // Struct
        let struct_items = vec![
            StructField {
                name: String::from("x"),
                decl: TypeUid::new(10),
            },
            StructField {
                name: String::from("y"),
                decl: TypeUid::new(11),
            },
        ];
        let def_struct = Definition::Struct {
            items: struct_items,
        };
        test_serialization_roundtrip(&def_struct);
    }

    #[test]
    fn entry_point_and_message_roundtrip() {
        let args = vec![TypeArgument {
            decl: TypeUid::new(100),
        }];
        let flags = TypeEntryPointFlags::IS_CONSTRUCTOR | TypeEntryPointFlags::IS_PAYABLE;
        let entry = TypeEntryPoint {
            name: String::from("init"),
            export_name: String::from("init_export"),
            arguments: args,
            result: TypeUid::new(101),
            flags,
        };
        test_serialization_roundtrip(&entry);

        let message = TypeMessage {
            topic: String::from("topic1"),
            decl: TypeUid::new(102),
        };
        test_serialization_roundtrip(&message);
    }
}
