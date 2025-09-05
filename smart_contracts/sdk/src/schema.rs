pub trait CasperSchema {
    fn schema() -> Schema;
}

use core::{any::TypeId, iter, mem, ptr::NonNull};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt::LowerHex,
};

use bitflags::Flags;
use casper_executor_wasm_common::{
    flags::EntryPointFlags,
    type_uid::{Uid, UidRepr},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::hash::{Hash, Hasher};

use crate::{
    abi::{ABITypeInfo, ABIVisitor, AbiDeclaration, Definition},
    abi_collector::{AbiItem, AbiReceiver, ABI_ITEMS},
    compat::types::CLType,
    serializers::AbiConvention,
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
    Contract { state: AbiDeclaration },
    /// Schemas of interface type does not contain state.
    Interface,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SchemaMessage {
    pub name: String,
    pub decl: AbiDeclaration,
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Schema {
    pub name: String,
    pub version: Option<String>,
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

    let mut cltypes = BTreeMap::new(); // typeid -> cltype
                                       // let mut defs = BTreeMap::new(); // typeid -> def

    let mut schema_decls = SchemaDeclarations::default();
    let mut schema_defs = SchemaDefinitions::default();
    let mut schema_entry = Vec::new();

    for abi_item in ABI_ITEMS.iter() {
        match abi_item {
            AbiItem::SmartContract(abi_smart_contract) => {}
            AbiItem::EntryPoint(abi_entry_point) => {
                // Collect types from params + result
                for abi_type in abi_entry_point
                    .params
                    .iter()
                    .map(|param| &param.decl)
                    .chain(iter::once(&abi_entry_point.result_decl))
                {
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
                        let type_uid = abi_type_info.type_uid().clone();
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

                let mut schema_params = Vec::new();

                for abi_type in abi_entry_point.params {
                    assert!(visited_types.contains(&abi_type.decl.type_id),);

                    schema_params.push(SchemaArgument {
                        name: abi_type.name.to_string(),
                        decl: SchemaTypeUid::from(abi_type.decl.type_id),
                    });
                }

                let receiver = match abi_entry_point.receiver {
                    Some(AbiReceiver::ByMutRef) => {
                        assert!(
                            !abi_entry_point.is_constructor,
                            "Constructor can not have &mut self"
                        );
                        Some(SchemaReceiver::Mutable)
                    }
                    Some(AbiReceiver::ByRef | AbiReceiver::ByVal) => {
                        assert!(
                            !abi_entry_point.is_constructor,
                            "Constructor can not have &self {abi_entry_point:?}"
                        );
                        Some(SchemaReceiver::Immutable)
                    }
                    None => {
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

    // let q = VecDeque::new();

    // for (type_id, schema_data) in defs {
    //     let cltype = cltypes.get(&type_id).expect("cltype for type_id").clone();

    //     dbg!(&schema_data.defs);

    //     let (type_id, decl, def) = schema_data.defs.first().expect("at least one def").clone();
    // // first item in the list is the definition of T.

    //     let schema_type_id = SchemaTypeId::try_from(type_id).unwrap();

    //     schema_decls.0.insert(decl, schema_type_id);
    //     schema_defs.0.insert(schema_type_id, SchemaDefinition { definition: def, cl_type: cltype
    // });

    //     // schema_defs.insert(type_id, (cltype, schema_data));
    // }

    // let SchemaData { defs, declarations, definitions, entry_points, messages, named_keys } =
    // schema_data;

    // let mut schema_defs = SchemaDefinitions::default();
    // for (decl, def) in defs {

    // }

    Schema {
        name: "contract".to_string(),
        version: None,
        type_: SchemaType::Contract {
            state: "Contract".to_string(),
        },
        declarations: schema_decls,
        definitions: schema_defs,
        entry_points: schema_entry,
        messages: Default::default(),
        named_keys: Default::default(),
    }
    // // Collect definitions
    // let definitions = {
    //     let mut definitions = Definitions::default();

    //     for abi_collector in ABI_COLLECTORS {
    //         abi_collector(&mut definitions);
    //     }

    //     definitions
    // };

    // // Collect messages
    // let messages = {
    //     let mut messages = Vec::new();

    //     for message in MESSAGES {
    //         messages.push(SchemaMessage {
    //             name: message.name.to_owned(),
    //             decl: message.decl.to_owned(),
    //         });
    //     }

    //     messages
    // };

    // // Collect named keys
    // let named_keys = {
    //     let mut named_keys = Vec::new();

    //     for named_key in NAMED_KEYS {
    //         named_keys.push(crate::schema::SchemaStableKey {
    //             name: named_key.name.to_owned(),
    //             decl: (named_key.decl)(),
    //         });
    //     }

    //     named_keys
    // };

    // // Collect entrypoints
    // let entry_points = {
    //     let mut entry_points = Vec::new();
    //     for entrypoint in ENTRYPOINTS {
    //         entry_points.push(entrypoint());
    //     }
    //     entry_points
    // };

    // // Construct a schema object from the extracted information
    // Schema {
    //     name: "contract".to_string(),
    //     version: None,
    //     type_: SchemaType::Contract {
    //         state: "Contract".to_string(),
    //     },
    //     definitions,
    //     entry_points,
    //     messages,
    //     named_keys,
    // }
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
