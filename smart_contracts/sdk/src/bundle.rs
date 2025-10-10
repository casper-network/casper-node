use crate::serializers::borsh::{BorshSerialize, BorshDeserialize};
use crate::{prelude::collections::BTreeMap, schema::Schema};

use serde::{Deserialize, Serialize};

use crate::{abi::Definition, compat::types::CLType, schema::SchemaUid};


#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleDefinition {
    pub definition: Definition,
    pub cl_type: CLType,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleArgument {
    pub decl: SchemaUid,
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
    pub result: SchemaUid,
    pub flags: BundleEntryPointFlags,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleMessage {
    /// The topic of the message.
    ///
    /// This, unlike the type names etc, is crucial for discovering messages.
    pub topic: String,
    pub decl: SchemaUid,
}

#[derive(Debug, PartialEq, Eq, Clone, BorshSerialize, BorshDeserialize)]
pub struct BundleV1 {
    definitions: BTreeMap<SchemaUid, BundleDefinition>,
    entry_points: Vec<BundleEntryPoint>,
    messages: Vec<BundleMessage>,
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
            .map(|(k, v)| (k, BundleDefinition { definition: v.definition, cl_type: v.cl_type }))
            .collect();

        let bundle_messages = messages
            .into_iter()
            .map(|msg| {
                BundleMessage {
                    topic: msg.topic,
                    decl: SchemaUid::from(msg.decl),
                }
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
                            // Although the macro does not perform read/write state operations (there's no self that we can dispatch entry points onto),
                            // we consider entry points without a receiver as mutable to allow state modifications via runtime functions.
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
                        .map(|arg| BundleArgument { decl: SchemaUid::from(arg.decl) })
                        .collect(),
                    result: SchemaUid::from(ep.result),
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
