use crate::{prelude::collections::BTreeMap, schema::Schema};

use serde::{Deserialize, Serialize};

use crate::{abi::Definition, compat::types::CLType, schema::SchemaUid};


#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BundleDefinition {
    pub definition: Definition,
    pub cl_type: CLType,
}

#[derive(Debug, PartialEq, Eq, Clone)]
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BundleEntryPoint {
    pub name: String,
    pub export_name: String,
    pub arguments: Vec<BundleArgument>,
    pub result: SchemaUid,
    pub flags: BundleEntryPointFlags,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Bundle {
    definitions: BTreeMap<SchemaUid, BundleDefinition>,
}

impl From<Schema> for Bundle {
    fn from(schema: Schema) -> Self {

        let Schema {
            definitions,
            metadata,
            type_,
            declarations,
            entry_points,
            messages,
            named_keys,
        } = schema;

        let bundle_definitions = definitions
            .0
            .into_iter()
            .map(|(k, v)| (k, BundleDefinition { definition: v.definition, cl_type: v.cl_type }))
            .collect();

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
                        None => {}
                    }
                    match ep.abi_convention {
                        crate::schema::EntryPointAbiConvention::Named => {
                            flags |= BundleEntryPointFlags::USES_NAMED_CONVENTION;
                        }
                        crate::schema::EntryPointAbiConvention::Positional => {
                            flags |= BundleEntryPointFlags::USES_POSITIONAL_CONVENTION;
                        }
                        crate::schema::EntryPointAbiConvention::Default => {
                            // This is the default behavior
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
        }
    }
}
