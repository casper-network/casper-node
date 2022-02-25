use std::collections::BTreeMap;

use parity_wasm::elements::Module;
use wasmi::{ImportsBuilder, MemoryRef, ModuleInstance, ModuleRef};

use casper_types::{
    contracts::NamedKeys, AccessRights, CLType, CLValue, Key, ProtocolVersion, PublicKey,
    RuntimeArgs, URef, URefAddr, U128, U256, U512,
};

use crate::{
    core::{
        execution::Error,
        resolvers::{self, memory_resolver::MemoryResolver},
    },
    shared::wasm_config::WasmConfig,
};

/// Creates an WASM module instance and a memory instance.
///
/// This ensures that a memory instance is properly resolved into a pre-allocated memory area, and a
/// host function resolver is attached to the module.
///
/// The WASM module is also validated to not have a "start" section as we currently don't support
/// running it.
///
/// Both [`ModuleRef`] and a [`MemoryRef`] are ready to be executed.
pub(super) fn instance_and_memory(
    parity_module: Module,
    protocol_version: ProtocolVersion,
    wasm_config: &WasmConfig,
) -> Result<(ModuleRef, MemoryRef), Error> {
    let module = wasmi::Module::from_parity_wasm_module(parity_module)?;
    let resolver = resolvers::create_module_resolver(protocol_version, wasm_config)?;
    let mut imports = ImportsBuilder::new();
    imports.push_resolver("env", &resolver);
    let not_started_module = ModuleInstance::new(&module, &imports)?;
    if not_started_module.has_start() {
        return Err(Error::UnsupportedWasmStart);
    }
    let instance = not_started_module.not_started_instance().clone();
    let memory = resolver.memory_ref()?;
    Ok((instance, memory))
}

/// Removes `rights_to_disable` from all urefs in `args` matching the address `uref_addr`.
pub(super) fn attenuate_uref_in_args(
    mut args: RuntimeArgs,
    uref_addr: URefAddr,
    rights_to_disable: AccessRights,
) -> Result<RuntimeArgs, Error> {
    for arg in args.named_args_mut() {
        *arg.cl_value_mut() = rewrite_urefs(arg.cl_value().clone(), |uref| {
            if uref.addr() == uref_addr {
                uref.disable_access_rights(rights_to_disable);
            }
        })?;
    }

    Ok(args)
}

/// Extracts a copy of every uref able to be deserialized from `cl_value`.
pub(super) fn extract_urefs(cl_value: &CLValue) -> Result<Vec<URef>, Error> {
    let mut vec: Vec<URef> = Vec::new();
    rewrite_urefs(cl_value.clone(), |uref| {
        vec.push(*uref);
    })?;
    Ok(vec)
}

/// Executes `func` on every uref able to be deserialized from `cl_value` and returns the resulting
/// re-serialized `CLValue`.
#[allow(clippy::cognitive_complexity)]
fn rewrite_urefs(cl_value: CLValue, mut func: impl FnMut(&mut URef)) -> Result<CLValue, Error> {
    let ret = match cl_value.cl_type() {
        CLType::Bool
        | CLType::I32
        | CLType::I64
        | CLType::U8
        | CLType::U32
        | CLType::U64
        | CLType::U128
        | CLType::U256
        | CLType::U512
        | CLType::Unit
        | CLType::String
        | CLType::PublicKey
        | CLType::Any => cl_value,
        CLType::Option(ty) => match **ty {
            CLType::URef => {
                let mut opt: Option<URef> = cl_value.to_owned().into_t()?;
                opt.iter_mut().for_each(func);
                CLValue::from_t(opt)?
            }
            CLType::Key => {
                let mut opt: Option<Key> = cl_value.to_owned().into_t()?;
                opt.iter_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(opt)?
            }
            _ => cl_value,
        },
        CLType::List(ty) => match **ty {
            CLType::URef => {
                let mut urefs: Vec<URef> = cl_value.to_owned().into_t()?;
                urefs.iter_mut().for_each(func);
                CLValue::from_t(urefs)?
            }
            CLType::Key => {
                let mut keys: Vec<Key> = cl_value.to_owned().into_t()?;
                keys.iter_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(keys)?
            }
            _ => cl_value,
        },
        CLType::ByteArray(_) => cl_value,
        CLType::Result { ok, err } => match (&**ok, &**err) {
            (CLType::URef, CLType::Bool) => {
                let mut res: Result<URef, bool> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::I32) => {
                let mut res: Result<URef, i32> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::I64) => {
                let mut res: Result<URef, i64> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::U8) => {
                let mut res: Result<URef, u8> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::U32) => {
                let mut res: Result<URef, u32> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::U64) => {
                let mut res: Result<URef, u64> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::U128) => {
                let mut res: Result<URef, U128> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::U256) => {
                let mut res: Result<URef, U256> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::U512) => {
                let mut res: Result<URef, U512> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::Unit) => {
                let mut res: Result<URef, ()> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::String) => {
                let mut res: Result<URef, String> = cl_value.to_owned().into_t()?;
                res.iter_mut().for_each(func);
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::Key) => {
                let mut res: Result<URef, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(uref) => func(uref),
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::URef, CLType::URef) => {
                let mut res: Result<URef, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(uref) => func(uref),
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::Bool) => {
                let mut res: Result<Key, bool> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::I32) => {
                let mut res: Result<Key, i32> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::I64) => {
                let mut res: Result<Key, i64> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::U8) => {
                let mut res: Result<Key, u8> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::U32) => {
                let mut res: Result<Key, u32> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::U64) => {
                let mut res: Result<Key, u64> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::U128) => {
                let mut res: Result<Key, U128> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::U256) => {
                let mut res: Result<Key, U256> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::U512) => {
                let mut res: Result<Key, U512> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::Unit) => {
                let mut res: Result<Key, ()> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::String) => {
                let mut res: Result<Key, String> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::URef) => {
                let mut res: Result<Key, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::Key, CLType::Key) => {
                let mut res: Result<Key, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(Key::URef(uref)) => func(uref),
                    Err(Key::URef(uref)) => func(uref),
                    Ok(_) | Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Bool, CLType::URef) => {
                let mut res: Result<bool, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::I32, CLType::URef) => {
                let mut res: Result<i32, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::I64, CLType::URef) => {
                let mut res: Result<i64, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::U8, CLType::URef) => {
                let mut res: Result<u8, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::U32, CLType::URef) => {
                let mut res: Result<u32, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::U64, CLType::URef) => {
                let mut res: Result<u64, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::U128, CLType::URef) => {
                let mut res: Result<U128, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::U256, CLType::URef) => {
                let mut res: Result<U256, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::U512, CLType::URef) => {
                let mut res: Result<U512, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::Unit, CLType::URef) => {
                let mut res: Result<(), URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::String, CLType::URef) => {
                let mut res: Result<String, URef> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(uref) => func(uref),
                }
                CLValue::from_t(res)?
            }
            (CLType::Bool, CLType::Key) => {
                let mut res: Result<bool, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::I32, CLType::Key) => {
                let mut res: Result<i32, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::I64, CLType::Key) => {
                let mut res: Result<i64, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::U8, CLType::Key) => {
                let mut res: Result<u8, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::U32, CLType::Key) => {
                let mut res: Result<u32, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::U64, CLType::Key) => {
                let mut res: Result<u64, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::U128, CLType::Key) => {
                let mut res: Result<U128, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::U256, CLType::Key) => {
                let mut res: Result<U256, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::U512, CLType::Key) => {
                let mut res: Result<U512, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::Unit, CLType::Key) => {
                let mut res: Result<(), Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (CLType::String, CLType::Key) => {
                let mut res: Result<String, Key> = cl_value.to_owned().into_t()?;
                match &mut res {
                    Ok(_) => {}
                    Err(Key::URef(uref)) => func(uref),
                    Err(_) => {}
                }
                CLValue::from_t(res)?
            }
            (_, _) => cl_value,
        },
        CLType::Map { key, value } => match (&**key, &**value) {
            (CLType::URef, CLType::Bool) => {
                let mut map: BTreeMap<URef, bool> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::I32) => {
                let mut map: BTreeMap<URef, i32> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::I64) => {
                let mut map: BTreeMap<URef, i64> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::U8) => {
                let mut map: BTreeMap<URef, u8> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::U32) => {
                let mut map: BTreeMap<URef, u32> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::U64) => {
                let mut map: BTreeMap<URef, u64> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::U128) => {
                let mut map: BTreeMap<URef, U128> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::U256) => {
                let mut map: BTreeMap<URef, U256> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::U512) => {
                let mut map: BTreeMap<URef, U512> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::Unit) => {
                let mut map: BTreeMap<URef, ()> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::String) => {
                let mut map: BTreeMap<URef, String> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        func(&mut k);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::Key) => {
                let mut map: BTreeMap<URef, Key> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, mut v)| {
                        func(&mut k);
                        v.as_uref_mut().iter_mut().for_each(|v| func(v));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::URef, CLType::URef) => {
                let mut map: BTreeMap<URef, URef> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, mut v)| {
                        func(&mut k);
                        func(&mut v);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::Bool) => {
                let mut map: BTreeMap<Key, bool> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::I32) => {
                let mut map: BTreeMap<Key, i32> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::I64) => {
                let mut map: BTreeMap<Key, i64> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::U8) => {
                let mut map: BTreeMap<Key, u8> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::U32) => {
                let mut map: BTreeMap<Key, u32> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::U64) => {
                let mut map: BTreeMap<Key, u64> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::U128) => {
                let mut map: BTreeMap<Key, U128> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::U256) => {
                let mut map: BTreeMap<Key, U256> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::U512) => {
                let mut map: BTreeMap<Key, U512> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::Unit) => {
                let mut map: BTreeMap<Key, ()> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::String) => {
                let mut map: BTreeMap<Key, String> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::URef) => {
                let mut map: BTreeMap<Key, URef> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, mut v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        func(&mut v);
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Key, CLType::Key) => {
                let mut map: BTreeMap<Key, Key> = cl_value.to_owned().into_t()?;
                map = map
                    .into_iter()
                    .map(|(mut k, mut v)| {
                        k.as_uref_mut().iter_mut().for_each(|k| func(k));
                        v.as_uref_mut().iter_mut().for_each(|v| func(v));
                        (k, v)
                    })
                    .collect();
                CLValue::from_t(map)?
            }
            (CLType::Bool, CLType::URef) => {
                let mut map: BTreeMap<bool, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::I32, CLType::URef) => {
                let mut map: BTreeMap<i32, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::I64, CLType::URef) => {
                let mut map: BTreeMap<i64, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U8, CLType::URef) => {
                let mut map: BTreeMap<u8, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U32, CLType::URef) => {
                let mut map: BTreeMap<u32, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U64, CLType::URef) => {
                let mut map: BTreeMap<u64, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U128, CLType::URef) => {
                let mut map: BTreeMap<U128, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U256, CLType::URef) => {
                let mut map: BTreeMap<U256, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U512, CLType::URef) => {
                let mut map: BTreeMap<U512, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::Unit, CLType::URef) => {
                let mut map: BTreeMap<(), URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::String, CLType::URef) => {
                let mut map: BTreeMap<String, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::PublicKey, CLType::URef) => {
                let mut map: BTreeMap<PublicKey, URef> = cl_value.to_owned().into_t()?;
                map.values_mut().for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::Bool, CLType::Key) => {
                let mut map: BTreeMap<bool, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::I32, CLType::Key) => {
                let mut map: BTreeMap<i32, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::I64, CLType::Key) => {
                let mut map: BTreeMap<i64, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U8, CLType::Key) => {
                let mut map: BTreeMap<u8, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U32, CLType::Key) => {
                let mut map: BTreeMap<u32, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U64, CLType::Key) => {
                let mut map: BTreeMap<u64, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U128, CLType::Key) => {
                let mut map: BTreeMap<U128, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U256, CLType::Key) => {
                let mut map: BTreeMap<U256, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::U512, CLType::Key) => {
                let mut map: BTreeMap<U512, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::Unit, CLType::Key) => {
                let mut map: BTreeMap<(), Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::String, CLType::Key) => {
                let mut map: NamedKeys = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (CLType::PublicKey, CLType::Key) => {
                let mut map: BTreeMap<PublicKey, Key> = cl_value.to_owned().into_t()?;
                map.values_mut().filter_map(Key::as_uref_mut).for_each(func);
                CLValue::from_t(map)?
            }
            (_, _) => cl_value,
        },
        CLType::Tuple1([ty]) => match **ty {
            CLType::URef => {
                let mut val: (URef,) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            CLType::Key => {
                let mut val: (Key,) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            _ => cl_value,
        },
        CLType::Tuple2([ty1, ty2]) => match (&**ty1, &**ty2) {
            (CLType::URef, CLType::Bool) => {
                let mut val: (URef, bool) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::I32) => {
                let mut val: (URef, i32) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::I64) => {
                let mut val: (URef, i64) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::U8) => {
                let mut val: (URef, u8) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::U32) => {
                let mut val: (URef, u32) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::U64) => {
                let mut val: (URef, u64) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::U128) => {
                let mut val: (URef, U128) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::U256) => {
                let mut val: (URef, U256) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::U512) => {
                let mut val: (URef, U512) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::Unit) => {
                let mut val: (URef, ()) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::String) => {
                let mut val: (URef, String) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::Key) => {
                let mut val: (URef, Key) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::URef, CLType::URef) => {
                let mut val: (URef, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.0);
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::Bool) => {
                let mut val: (Key, bool) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::I32) => {
                let mut val: (Key, i32) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::I64) => {
                let mut val: (Key, i64) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::U8) => {
                let mut val: (Key, u8) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::U32) => {
                let mut val: (Key, u32) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::U64) => {
                let mut val: (Key, u64) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::U128) => {
                let mut val: (Key, U128) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::U256) => {
                let mut val: (Key, U256) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::U512) => {
                let mut val: (Key, U512) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::Unit) => {
                let mut val: (Key, ()) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::String) => {
                let mut val: (Key, String) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::URef) => {
                let mut val: (Key, URef) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::Key, CLType::Key) => {
                let mut val: (Key, Key) = cl_value.to_owned().into_t()?;
                val.0.as_uref_mut().iter_mut().for_each(|v| func(v));
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Bool, CLType::URef) => {
                let mut val: (bool, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::I32, CLType::URef) => {
                let mut val: (i32, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::I64, CLType::URef) => {
                let mut val: (i64, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::U8, CLType::URef) => {
                let mut val: (u8, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::U32, CLType::URef) => {
                let mut val: (u32, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::U64, CLType::URef) => {
                let mut val: (u64, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::U128, CLType::URef) => {
                let mut val: (U128, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::U256, CLType::URef) => {
                let mut val: (U256, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::U512, CLType::URef) => {
                let mut val: (U512, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::Unit, CLType::URef) => {
                let mut val: ((), URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::String, CLType::URef) => {
                let mut val: (String, URef) = cl_value.to_owned().into_t()?;
                func(&mut val.1);
                CLValue::from_t(val)?
            }
            (CLType::Bool, CLType::Key) => {
                let mut val: (bool, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::I32, CLType::Key) => {
                let mut val: (i32, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::I64, CLType::Key) => {
                let mut val: (i64, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::U8, CLType::Key) => {
                let mut val: (u8, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::U32, CLType::Key) => {
                let mut val: (u32, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::U64, CLType::Key) => {
                let mut val: (u64, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::U128, CLType::Key) => {
                let mut val: (U128, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::U256, CLType::Key) => {
                let mut val: (U256, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::U512, CLType::Key) => {
                let mut val: (U512, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::Unit, CLType::Key) => {
                let mut val: ((), Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (CLType::String, CLType::Key) => {
                let mut val: (String, Key) = cl_value.to_owned().into_t()?;
                val.1.as_uref_mut().iter_mut().for_each(|v| func(v));
                CLValue::from_t(val)?
            }
            (_, _) => cl_value,
        },
        // TODO: nested matches for Tuple3?
        CLType::Tuple3(_) => cl_value,
        CLType::Key => {
            let mut key: Key = cl_value.to_owned().into_t()?; // TODO: optimize?
            key.as_uref_mut().iter_mut().for_each(|v| func(v));
            CLValue::from_t(key)?
        }
        CLType::URef => {
            let mut uref: URef = cl_value.to_owned().into_t()?; // TODO: optimize?
            func(&mut uref);
            CLValue::from_t(uref)?
        }
    };
    Ok(ret)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::{
        array::uniform32,
        collection::{btree_map, vec},
        option,
        prelude::*,
        result,
    };

    use casper_types::{
        gens::*, runtime_args, AccessRights, CLType, CLValue, Key, PublicKey, RuntimeArgs,
        SecretKey, URef,
    };

    use super::*;

    fn cl_value_with_urefs_arb() -> impl Strategy<Value = (CLValue, Vec<URef>)> {
        // If compiler brings you here it most probably means you've added a variant to `CLType`
        // enum but forgot to add generator for it.
        let stub: Option<CLType> = None;
        if let Some(cl_type) = stub {
            match cl_type {
                CLType::Bool
                | CLType::I32
                | CLType::I64
                | CLType::U8
                | CLType::U32
                | CLType::U64
                | CLType::U128
                | CLType::U256
                | CLType::U512
                | CLType::Unit
                | CLType::String
                | CLType::Key
                | CLType::URef
                | CLType::Option(_)
                | CLType::List(_)
                | CLType::ByteArray(..)
                | CLType::Result { .. }
                | CLType::Map { .. }
                | CLType::Tuple1(_)
                | CLType::Tuple2(_)
                | CLType::Tuple3(_)
                | CLType::PublicKey
                | CLType::Any => (),
            }
        };

        prop_oneof![
            Just((CLValue::from_t(()).expect("should create CLValue"), vec![])),
            any::<bool>()
                .prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            any::<i32>().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            any::<i64>().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            any::<u8>().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            any::<u32>().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            any::<u64>().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            u128_arb().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            u256_arb().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            u512_arb().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            key_arb().prop_map(|x| {
                let urefs = x.as_uref().into_iter().cloned().collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            uref_arb().prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![x])),
            ".*".prop_map(|x: String| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            option::of(any::<u64>())
                .prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            option::of(uref_arb()).prop_map(|x| {
                let urefs = x.iter().cloned().collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            option::of(key_arb()).prop_map(|x| {
                let urefs = x.iter().filter_map(Key::as_uref).cloned().collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            vec(any::<i32>(), 0..100)
                .prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            vec(uref_arb(), 0..100).prop_map(|x| (
                CLValue::from_t(x.clone()).expect("should create CLValue"),
                x
            )),
            vec(key_arb(), 0..100).prop_map(|x| (
                CLValue::from_t(x.clone()).expect("should create CLValue"),
                x.into_iter().filter_map(Key::into_uref).collect()
            )),
            uniform32(any::<u8>())
                .prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            result::maybe_err(key_arb(), ".*").prop_map(|x| {
                let urefs = match &x {
                    Ok(key) => key.as_uref().into_iter().cloned().collect(),
                    Err(_) => vec![],
                };
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            result::maybe_ok(".*", uref_arb()).prop_map(|x| {
                let urefs = match &x {
                    Ok(_) => vec![],
                    Err(uref) => vec![*uref],
                };
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            btree_map(".*", u512_arb(), 0..100)
                .prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            btree_map(uref_arb(), u512_arb(), 0..100).prop_map(|x| {
                let urefs = x.keys().cloned().collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            btree_map(".*", uref_arb(), 0..100).prop_map(|x| {
                let urefs = x.values().cloned().collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            btree_map(uref_arb(), key_arb(), 0..100).prop_map(|x| {
                let urefs: Vec<URef> = x
                    .clone()
                    .into_iter()
                    .flat_map(|(k, v)| {
                        vec![Some(k), v.into_uref()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<URef>>()
                    })
                    .collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            btree_map(key_arb(), uref_arb(), 0..100).prop_map(|x| {
                let urefs: Vec<URef> = x
                    .clone()
                    .into_iter()
                    .flat_map(|(k, v)| {
                        vec![k.into_uref(), Some(v)]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<URef>>()
                    })
                    .collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            btree_map(key_arb(), key_arb(), 0..100).prop_map(|x| {
                let urefs: Vec<URef> = x
                    .clone()
                    .into_iter()
                    .flat_map(|(k, v)| {
                        vec![k.into_uref(), v.into_uref()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<URef>>()
                    })
                    .collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            (any::<bool>())
                .prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            (uref_arb())
                .prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![x])),
            (any::<bool>(), any::<i32>())
                .prop_map(|x| (CLValue::from_t(x).expect("should create CLValue"), vec![])),
            (uref_arb(), any::<i32>()).prop_map(|x| {
                let uref = x.0;
                (
                    CLValue::from_t(x).expect("should create CLValue"),
                    vec![uref],
                )
            }),
            (any::<i32>(), key_arb()).prop_map(|x| {
                let urefs = x.1.as_uref().into_iter().cloned().collect();
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
            (uref_arb(), key_arb()).prop_map(|x| {
                let mut urefs = vec![x.0];
                urefs.extend(x.1.as_uref().into_iter().cloned());
                (CLValue::from_t(x).expect("should create CLValue"), urefs)
            }),
        ]
    }

    proptest! {
        #[test]
        fn should_extract_urefs((cl_value, urefs) in cl_value_with_urefs_arb()) {
            let extracted_urefs = extract_urefs(&cl_value).unwrap();
            prop_assert_eq!(extracted_urefs, urefs);
        }
    }

    #[test]
    fn extract_from_public_keys_to_urefs_map() {
        let uref = URef::new([43; 32], AccessRights::READ_ADD_WRITE);
        let mut map = BTreeMap::new();
        map.insert(
            PublicKey::from(
                &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            uref,
        );
        let cl_value = CLValue::from_t(map).unwrap();
        assert_eq!(extract_urefs(&cl_value).unwrap(), vec![uref]);
    }

    #[test]
    fn extract_from_public_keys_to_uref_keys_map() {
        let uref = URef::new([43; 32], AccessRights::READ_ADD_WRITE);
        let key = Key::from(uref);
        let mut map = BTreeMap::new();
        map.insert(
            PublicKey::from(
                &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
            ),
            key,
        );
        let cl_value = CLValue::from_t(map).unwrap();
        assert_eq!(extract_urefs(&cl_value).unwrap(), vec![uref]);
    }

    #[test]
    fn should_modify_urefs() {
        let uref_1 = URef::new([1; 32], AccessRights::READ_ADD_WRITE);
        let uref_2 = URef::new([2; 32], AccessRights::READ_ADD_WRITE);
        let uref_3 = URef::new([3; 32], AccessRights::READ_ADD_WRITE);

        let args = runtime_args! {
            "uref1" => uref_1,
            "uref2" => Some(uref_1),
            "uref3" => vec![uref_2, uref_1, uref_3],
            "uref4" => vec![Key::from(uref_3), Key::from(uref_2), Key::from(uref_1)],
        };

        let args = attenuate_uref_in_args(args, uref_1.addr(), AccessRights::WRITE).unwrap();

        let arg = args.get("uref1").unwrap().clone();
        let lhs = arg.into_t::<URef>().unwrap();
        let rhs = uref_1.with_access_rights(AccessRights::READ_ADD);
        assert_eq!(lhs, rhs);

        let arg = args.get("uref2").unwrap().clone();
        let lhs = arg.into_t::<Option<URef>>().unwrap();
        let rhs = uref_1.with_access_rights(AccessRights::READ_ADD);
        assert_eq!(lhs, Some(rhs));

        let arg = args.get("uref3").unwrap().clone();
        let lhs = arg.into_t::<Vec<URef>>().unwrap();
        let rhs = vec![
            uref_2.with_access_rights(AccessRights::READ_ADD_WRITE),
            uref_1.with_access_rights(AccessRights::READ_ADD),
            uref_3.with_access_rights(AccessRights::READ_ADD_WRITE),
        ];
        assert_eq!(lhs, rhs);

        let arg = args.get("uref4").unwrap().clone();
        let lhs = arg.into_t::<Vec<Key>>().unwrap();
        let rhs = vec![
            Key::from(uref_3.with_access_rights(AccessRights::READ_ADD_WRITE)),
            Key::from(uref_2.with_access_rights(AccessRights::READ_ADD_WRITE)),
            Key::from(uref_1.with_access_rights(AccessRights::READ_ADD)),
        ];
        assert_eq!(lhs, rhs);
    }
}
