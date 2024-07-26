use std::fmt::{self, Display, Formatter};

use crate::FIELD_INDEX_ATTRIBUTE;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParsingError {
    MalfrormedCalltableAttribute {
        attribute_name: String,
        field_name: String,
        got: String,
    },
    MultipleBinaryAttributes {
        field_name: String,
    },
    MultipleIndexUsages {
        field_name: String,
        index: u16,
    },
    BinaryAttributeMissing {
        field_name: String,
    },
    UnsupportedDataType {
        data_type: String,
    },
    UnexpectedBinaryIndexDefinition {
        attribute_name: String,
        field_name: String,
        got: String,
    },
    IndexValueNotU16 {
        field_name: String,
        got: String,
        err: String,
    },
    SkipNotAllowedForEnum {
        variant_name: String,
    },
    EnumCannotBeEmpty {
        enum_name: String,
    },
    EnumVariantIndexesNotSequential {
        enum_name: String,
        found_indexes: Vec<u8>,
        expected_indexes: Vec<u8>,
    },
    EnumVariantFieldIndexesNotSequential {
        enum_name: String,
        variant_name: String,
        found_indexes: Vec<u16>,
        expected_indexes: Vec<u16>,
    },
    StructFieldIndexesNotSequential {
        struct_name: String,
        found_indexes: Vec<u16>,
        expected_indexes: Vec<u16>,
    },
    EnumVariantIndexesNotUnique {
        variant_name: String,
        offending_index: u16,
    },
}

impl Display for ParsingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ParsingError::MultipleBinaryAttributes { field_name } => {
                write!(
                    formatter,
                    "Field {} has multiple {} attributes",
                    field_name, FIELD_INDEX_ATTRIBUTE
                )
            }
            ParsingError::MultipleIndexUsages { field_name, index } => {
                write!(
                    formatter,
                    "Field {} uses an index that was already used: {}. Indexes need to be unique in scope of a type definition.",
                    field_name, index
                )
            }
            ParsingError::BinaryAttributeMissing { field_name } => {
                write!(
                    formatter,
                    "Field {} is missing {}. If you don't want to use binary serialization for this field, use `#[calltable(skip)]`",
                    field_name,
                    FIELD_INDEX_ATTRIBUTE
                )
            }
            ParsingError::UnsupportedDataType { data_type } => {
                write!(formatter, "Unsupported type of data: {}", data_type)
            }
            ParsingError::UnexpectedBinaryIndexDefinition { attribute_name, field_name, got } => {
                write!(
                    formatter,
                    "Field {}. Expected either `#[calltable({} = <u16>)]` or `#[calltable(skip)]`. Got {}",
                    field_name,
                    attribute_name,
                    got
                )
            }
            ParsingError::IndexValueNotU16 {
                field_name,
                got,
                err,
            } => {
                write!(
                    formatter,
                    "Field {}. got a index of value {:?} that couldn't be interpreted as u16. Err: {}",
                    field_name,
                    got,
                    err
                )
            }
            ParsingError::SkipNotAllowedForEnum { variant_name } => {
                write!(
                    formatter,
                    "Variant {} has #[calltable(skip)] definition which is not allowed on an enum variant",
                    variant_name,
                )
            }
            ParsingError::EnumCannotBeEmpty { enum_name } => write!(
                formatter,
                "Enum {} must have at least one variant to be used with binary serialization",
                enum_name,
            ),
            ParsingError::EnumVariantIndexesNotSequential {
                enum_name,
                found_indexes,
                expected_indexes,
            } => write!(
                formatter,
                "Parsing enum {}. Binary indexes of variants must be sequential starting from 0. Found indexes {:?} but expected indexes {:?}",
                enum_name,
                found_indexes,
                expected_indexes
            ),
            ParsingError::EnumVariantFieldIndexesNotSequential { enum_name, variant_name, found_indexes, expected_indexes } =>
            write!(
                formatter,
                "Parsing enum variant {}::{}. Binary indexes of variant fields must be sequential starting from 1 (0 is reserved for variant tag). Found indexes {:?} but expected indexes {:?}",
                enum_name,
                variant_name,
                found_indexes,
                expected_indexes
            ),
            ParsingError::StructFieldIndexesNotSequential { struct_name, found_indexes, expected_indexes } => write!(
                formatter,
                "Parsing struct {}. Field indexes of variant fields must be sequential starting from 0. Found indexes {:?} but expected indexes {:?}",
                struct_name,
                found_indexes,
                expected_indexes
            ),
            ParsingError::MalfrormedCalltableAttribute { attribute_name, field_name, got } => write!(
                formatter,
                "Parsing field {}. Expected attribute \"calltable\" to be in form \"#[calltable({} = <u16>)]\" or \"#[calltable(skip)]\". Got: {}",
                field_name,
                attribute_name,
                got
            ),
            ParsingError::EnumVariantIndexesNotUnique { variant_name, offending_index } => write!(
                formatter,
                "Parsing field {}. Expected variant indexes to be unique, found duplicate {}",
                variant_name,
                offending_index
            ),
        }
    }
}
