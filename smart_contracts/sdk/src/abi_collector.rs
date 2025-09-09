use std::collections::BTreeMap;

use casper_executor_wasm_common::type_uid::Uid;

use crate::{
    abi::{ABIVisitor, AbiDeclaration},
    compat::types::CLType,
    linkme::distributed_slice,
    serializers::AbiConvention,
};

#[derive(Debug)]
pub struct AbiType {
    pub type_name: fn() -> &'static str,
    /// Unique identifier of the type found in the source code.
    pub type_id: Uid,
    pub cl_type: fn() -> CLType,
    pub visit_abi_types: fn(&mut dyn ABIVisitor) -> (),
}

impl AbiType {
    #[inline]
    pub fn cl_type(&self) -> CLType {
        (self.cl_type)()
    }
}
#[derive(Debug)]
pub struct AbiParam {
    pub name: &'static str,
    pub decl: AbiType,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AbiLocation {
    pub file: &'static str,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AbiKind {
    /// struct SmartContract {}
    SmartContract { struct_name: &'static str },
    /// impl `trait_name` for
    TraitImpl {
        struct_name: &'static str,
        trait_name: &'static str,
    },
    /// fn call() {}
    Function,
}

#[derive(Debug)]
pub enum AbiReceiver {
    /// Represents lack of smart contract state object.
    ///
    /// Having this instead wrapping AbiReceiver in None/Some simplifies the consumption of the
    /// macro-generated trait representation.
    NoReceiver,
    /// &self.
    ByRef,
    /// &mut self.
    ByMutRef,
    /// self.
    ByVal,
}

#[derive(Debug)]
pub struct AbiEntryPoint {
    /// Function name
    pub name: &'static str,
    /// Export name
    ///
    /// This may be different than `name` in case of trait impl methods.
    pub export_name: &'static str,
    pub receiver: AbiReceiver,
    pub params: &'static [AbiParam],

    pub abi_convention: AbiConvention,
    pub is_constructor: bool,
    pub is_payable: bool,

    pub result_decl: AbiType,
    pub kind: AbiKind,

    pub location: AbiLocation,
    /// The actual callable export
    pub fptr: fn() -> (),
}

#[derive(Debug)]
pub struct AbiSmartContract {
    pub struct_name: &'static str,
    pub abi_convention: AbiConvention,
    pub decl: AbiType,
    pub metadata: fn() -> BTreeMap<&'static str, Option<&'static str>>,
}

#[derive(Debug)]
pub enum AbiItem {
    /// Smart Contract
    ///
    /// #[casper(contract_state)]
    SmartContract(AbiSmartContract),
    /// impl Foo { fn method(params) -> result_decl }
    EntryPoint(AbiEntryPoint),
    /// #[casper(message)] struct Msg {}
    Message(AbiMessage),
}

#[derive(Debug)]
pub struct AbiMessage {
    /// This by default is the 'struct name' but may be changed into any other name as desired.
    pub name: fn() -> &'static str,
    pub decl: AbiType,
}

impl AbiItem {
    pub fn as_smart_contract(&self) -> Option<&AbiSmartContract> {
        match self {
            AbiItem::SmartContract(smart_contract) => Some(smart_contract),
            _ => None,
        }
    }

    pub fn as_entry_point(&self) -> Option<&AbiEntryPoint> {
        match self {
            AbiItem::EntryPoint(entry_point) => Some(entry_point),
            _ => None,
        }
    }

    pub fn as_message(&self) -> Option<&AbiMessage> {
        match self {
            AbiItem::Message(message) => Some(message),
            _ => None,
        }
    }
}

#[distributed_slice]
#[linkme(crate = crate::linkme)]
pub static ABI_ITEMS: [AbiItem] = [..];

#[derive(Debug, Clone)]
pub struct NamedKey {
    pub name: &'static str,
    pub decl: fn() -> AbiDeclaration,
}

#[distributed_slice]
#[linkme(crate = crate::linkme)]
pub static NAMED_KEYS: [NamedKey] = [..];
