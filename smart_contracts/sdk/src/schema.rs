pub trait CasperSchema {
    fn schema() -> Schema;
}

use crate::prelude::{
    collections::{BTreeMap, BTreeSet},
    fmt::LowerHex,
    String, ToString, Vec,
};
use core::{mem, ptr::NonNull};

use bitflags::Flags;
use casper_executor_wasm_common::type_uid::{Uid, UidRepr};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    abi::{ABITypeInfo, ABIVisitor, AbiDeclaration, Definition},
    abi_collector::{AbiItem, AbiReceiver, ABI_ITEMS},
    compat::types::CLType,
};

pub fn serialize_bits<T, S>(data: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Flags,
    T::Bits: Serialize,
{
    data.bits().serialize(serializer)
}

pub fn deserialize_bits<'de, D, F>(deserializer: D) -> Result<F, D::Error>
where
    D: Deserializer<'de>,
    F: Flags,
    F::Bits: Deserialize<'de> + LowerHex,
{
    let raw: F::Bits = F::Bits::deserialize(deserializer)?;
    F::from_bits(raw).ok_or(serde::de::Error::custom(format!(
        "Unexpected flags value 0x{raw:#08x}"
    )))
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SchemaArgument {
    pub name: String,
    pub decl: SchemaTypeUid,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum SchemaReceiver {
    /// This entry point mutates state (in terms of Rust source code it means given entry point
    /// uses `&mut self`)
    Mutable,
    /// This entry point does not mutate state (in terms of Rust source code it means given entry
    /// point uses `&self` or `self`)
    Immutable,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SchemaEntryPoint {
    /// Actual name of the entry point (i.e. balance_of)
    ///
    /// For consumers of the schema this is the name to use when code generating a client (i.e.
    /// `client.balance_of`)
    pub name: String,
    /// Name of the export as present in the Wasm bytecode. This is the name that is expected by
    /// the system to call a given smart contract.
    ///
    /// A codegen may want to decide to call `export_name` but refer to given entry point by the
    /// `name` (i.e. `client.balance_of` signs a transaction that calls stored smart contract by
    /// `client.CEP18_balance_of`). Generally, regardless of the tech stack used to produce
    /// given smart contract, a `name` field may not equal to the `export_name`. Examples: Rust
    /// trait system prefixes Wasm exports with the trait name, but keeps the original method name
    /// as the `name`.
    pub export_name: String,
    pub arguments: Vec<SchemaArgument>,
    pub result: SchemaTypeUid,
    /// Receiver of given entrypoint in terms of source code i.e. `&self` or `&mut self` which in
    /// case of Rust SDK means it mutates state.
    pub receiver: Option<SchemaReceiver>,
    /// Whether this entry point is a constructor.
    ///
    /// Implies `receiver` is not specified.
    pub is_constructor: bool,
    /// Whether this entry point can receive payments.
    pub is_payable: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum SchemaAbiConvention {
    Named,
    Positional,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
#[serde(tag = "type")]
pub enum SchemaType {
    /// Contract schemas contain a state structure that we want to mark in the schema.
    Contract { state: SchemaTypeUid },
    /// Schemas of interface type does not contain state.
    Interface,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SchemaMessage {
    pub name: String,
    pub decl: SchemaTypeUid,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SchemaStableKey {
    pub name: String,
    pub decl: AbiDeclaration,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Hash)]
pub struct SchemaTypeUid(UidRepr);

impl From<Uid> for SchemaTypeUid {
    fn from(uid: Uid) -> Self {
        SchemaTypeUid(uid.into_raw())
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
pub struct SchemaDeclarations(BTreeMap<SchemaTypeUid, String>);

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SchemaDefinition {
    pub definition: Definition,
    pub cl_type: CLType,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
pub struct SchemaDefinitions(BTreeMap<SchemaTypeUid, SchemaDefinition>);

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
pub struct SchemaCLTypes(BTreeMap<String, CLType>);

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SchemaMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Schema {
    pub metadata: SchemaMetadata,
    #[serde(rename = "type")]
    pub type_: SchemaType,
    pub declarations: SchemaDeclarations,
    pub definitions: SchemaDefinitions,
    pub entry_points: Vec<SchemaEntryPoint>,
    pub messages: Vec<SchemaMessage>,
    pub named_keys: Vec<SchemaStableKey>,
}

#[derive(Debug)]
struct SchemaData {
    defs: Vec<ABITypeInfo>,
}

impl ABIVisitor for SchemaData {
    fn accept(&mut self, type_info: ABITypeInfo) {
        self.defs.push(type_info);
    }
}

pub fn casper_collect_schema() -> Schema {
    let mut visited_types = BTreeSet::new();

    let mut cltypes = BTreeMap::new();

    let mut schema_decls = SchemaDeclarations::default();
    let mut schema_defs = SchemaDefinitions::default();
    let mut schema_entry = Vec::new();
    let mut schema_messages = Vec::new();

    let mut abi_types = Vec::new();
    for abi_item in ABI_ITEMS.iter() {
        match abi_item {
            AbiItem::Message(abi_message) => {
                abi_types.push(&abi_message.decl);
            }
            AbiItem::SmartContract(abi_smart_contract) => {
                abi_types.push(&abi_smart_contract.decl);
            }
            AbiItem::EntryPoint(abi_entry_point) => {
                for param in abi_entry_point.params {
                    abi_types.push(&param.decl);
                }
                abi_types.push(&abi_entry_point.result_decl);
            }
        }
    }

    let smart_contracts = ABI_ITEMS
        .iter()
        .filter_map(AbiItem::as_smart_contract)
        .collect::<Vec<_>>();
    assert_eq!(
        smart_contracts.len(),
        1,
        "Expected exactly one smart contract in the ABI_ITEMS, found {}",
        smart_contracts.len()
    );

    let smart_contract = smart_contracts
        .into_iter()
        .next()
        .expect("Failed to get smart contract");

    // Collect types from params + result
    for abi_type in abi_types {
        let param_type_id = abi_type.type_id;
        if visited_types.contains(&param_type_id) {
            // Type already seen
            continue;
        }

        let cl_type = (abi_type.cl_type)();

        cltypes.insert(param_type_id, cl_type.clone());

        let mut schema_data = SchemaData {
            defs: Default::default(),
        };

        (abi_type.visit_abi_types)(&mut schema_data);

        assert_eq!(
            schema_data.defs.first().as_ref().unwrap().type_uid(),
            param_type_id,
            "parameter type ID mismatch decl={:?} {:#?} {:#?}", /* means CasperABI
                                                                 * implementation is
                                                                 * incorrect */
            (abi_type.type_name)(),
            schema_data.defs,
            schema_decls,
        );

        for abi_type_info in schema_data.defs {
            let type_uid = abi_type_info.type_uid();
            let cl_type = abi_type_info.cl_type().clone();
            let decl = abi_type_info.declaration().clone();
            let def = abi_type_info.definition().clone();

            schema_decls
                .0
                .insert(SchemaTypeUid::from(type_uid), decl.clone());
            schema_defs.0.insert(
                SchemaTypeUid::from(type_uid),
                SchemaDefinition {
                    definition: def,
                    cl_type,
                },
            );
        }

        visited_types.insert(param_type_id);
    }

    for abi_item in ABI_ITEMS.iter() {
        match abi_item {
            AbiItem::Message(abi_message) => {
                // Process message

                schema_messages.push(SchemaMessage {
                    name: (abi_message.name)().to_string(),
                    decl: SchemaTypeUid::from(abi_message.decl.type_id),
                });
            }
            AbiItem::SmartContract(_abi_smart_contract) => {}
            AbiItem::EntryPoint(abi_entry_point) => {
                let mut schema_params = Vec::new();

                for abi_type in abi_entry_point.params {
                    assert!(visited_types.contains(&abi_type.decl.type_id),);

                    schema_params.push(SchemaArgument {
                        name: abi_type.name.to_string(),
                        decl: SchemaTypeUid::from(abi_type.decl.type_id),
                    });
                }

                let receiver = match abi_entry_point.receiver {
                    AbiReceiver::ByMutRef => {
                        assert!(
                            !abi_entry_point.is_constructor,
                            "Constructor can not have &mut self"
                        );
                        Some(SchemaReceiver::Mutable)
                    }
                    AbiReceiver::ByRef | AbiReceiver::ByVal => {
                        assert!(
                            !abi_entry_point.is_constructor,
                            "Constructor can not have &self {abi_entry_point:?}"
                        );
                        Some(SchemaReceiver::Immutable)
                    }
                    AbiReceiver::NoReceiver => {
                        // No receiver; treat as immutable. May or may not be constructor.
                        None
                    }
                };

                let schema_entrypoint = SchemaEntryPoint {
                    name: abi_entry_point.name.to_string(),
                    arguments: schema_params,
                    result: SchemaTypeUid::from(abi_entry_point.result_decl.type_id),
                    export_name: abi_entry_point.export_name.to_string(),
                    receiver,
                    is_constructor: abi_entry_point.is_constructor,
                    is_payable: abi_entry_point.is_payable,
                };
                schema_entry.push(schema_entrypoint);
            }
        }
    }

    let metadata = (smart_contract.metadata)();

    let schema_metadata = SchemaMetadata {
        name: metadata["CARGO_PKG_NAME"].map(ToOwned::to_owned),
        version: metadata["CARGO_PKG_VERSION"].map(ToOwned::to_owned),
        authors: metadata["CARGO_PKG_AUTHORS"].map(|s| {
            s.split(':')
                .filter(|part| !part.is_empty())
                .map(|part| part.to_string())
                .collect::<Vec<String>>()
        }),
        description: metadata["CARGO_PKG_DESCRIPTION"].map(ToOwned::to_owned),
        rust_version: metadata["CARGO_PKG_RUST_VERSION"].map(ToOwned::to_owned),
    };

    Schema {
        metadata: schema_metadata,
        type_: SchemaType::Contract {
            state: SchemaTypeUid::from(smart_contract.decl.type_id),
        },
        declarations: schema_decls,
        definitions: schema_defs,
        entry_points: schema_entry,
        messages: schema_messages,
        named_keys: Default::default(),
    }
}

/// This function is called by the host to collect the schema from the contract.
///
/// This is considered internal implementation detail and should not be used directly.
/// Primary user of this API is `cargo-casper` tool that will use it to extract schema from the
/// contract.
///
/// # Safety
/// Pointer to json bytes passed to the callback is valid only within the scope of that function.
#[export_name = "__cargo_casper_collect_schema"]
pub unsafe extern "C" fn cargo_casper_collect_schema(size_ptr: *mut u64) -> *mut u8 {
    let schema = casper_collect_schema();
    // Write the schema using the provided writer
    let mut json_bytes = serde_json::to_vec_pretty(&schema).expect("Serialized schema");
    NonNull::new(size_ptr)
        .expect("expected non-null ptr")
        .write(json_bytes.len().try_into().expect("usize to u64"));
    let ptr = json_bytes.as_mut_ptr();
    mem::forget(json_bytes);
    ptr
}
