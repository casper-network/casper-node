pub(crate) mod control;
pub(crate) mod crypto;
pub(crate) mod emit;
pub(crate) mod global_state;
pub(crate) mod io;
use std::sync::Arc;

use bytes::Bytes;
use casper_executor_wasm_common::error::CallError;

use casper_executor_wasm_interface::{
    executor::{
        ControlMethods, CryptoMethods, EmitMethods, ExecuteError, ExecuteRequestBuilder,
        ExecuteResult, ExecutionKind, Executor, GlobalStateMethods, IOMethods, SystemContractMenu,
    },
    u32_from_host_result, Caller, FatalHostError, VMError, VMResult,
};
use casper_storage::global_state::GlobalStateReader;
use casper_types::{bytesrepr::ToBytes, BlockHash, Digest, EntityAddr, Key, StoredValue};
use num_derive::FromPrimitive;
use tracing::error;

use crate::{
    context::Context,
    host::{
        control::{host_call, host_upgrade},
        crypto::{
            host_alt_bn128_add, host_alt_bn128_mul, host_alt_bn128_pairing, host_generic_hash,
            host_recover_secp256k1,
        },
        emit::{emit, print_std},
        global_state::{
            host_create, host_env_balance, host_env_info, host_read, host_remove, host_write,
        },
        io::{host_copy_input, host_return},
    },
};
use casper_executor_wasm_interface::executor::{ExecuteRequest, FFIMenu};

const NAME_FOR_V2_CONTRACT_MAIN_PURSE: &str = "__main_purse";

#[derive(Debug, Copy, Clone, FromPrimitive, PartialEq)]
enum EntityKindTag {
    Account = 0,
    Contract = 1,
}

pub trait FallibleInto<T> {
    fn wrapped_try_into(self) -> VMResult<T>;
}

impl<From, To> FallibleInto<To> for From
where
    To: TryFrom<From>,
{
    fn wrapped_try_into(self) -> VMResult<To> {
        To::try_from(self).map_err(|_| VMError::Fatal(FatalHostError::TypeConversion))
    }
}

/// Consumes imputed amount of gas.
fn charge_gas<S: GlobalStateReader>(
    caller: &mut impl Caller<Context = Context<S>>,
    imputed: u64,
) -> VMResult<()> {
    caller.consume_gas(imputed)?;
    Ok(())
}

/// Consumes a set amount of gas for the specified storage value.
fn charge_gas_storage<S: GlobalStateReader>(
    caller: &mut impl Caller<Context = Context<S>>,
    size_bytes: usize,
) -> VMResult<()> {
    let storage_costs = &caller.context().storage_costs;
    let gas_cost = storage_costs.calculate_gas_cost(size_bytes);
    let value: u64 = gas_cost.value().try_into().map_err(|_| VMError::OutOfGas)?;
    caller.consume_gas(value)?;
    Ok(())
}

/// Writes a message to the global state and charges for storage used.
fn metered_write<S: GlobalStateReader>(
    caller: &mut impl Caller<Context = Context<S>>,
    key: Key,
    value: StoredValue,
) -> VMResult<()> {
    if caller.context().sandboxed {
        return Err(VMError::Execute(ExecuteError::AttemptWriteInRestricted));
    }

    charge_gas_storage(caller, value.serialized_length())?;
    caller.context_mut().tracking_copy.write(key, value);
    Ok(())
}

fn context_to_entity_addr<S: GlobalStateReader>(context: &Context<S>) -> EntityAddr {
    match context.callee {
        Key::Account(account_hash) => EntityAddr::new_account(account_hash.value()),
        Key::Hash(hash_addr) => EntityAddr::SmartContract(hash_addr),
        Key::AddressableEntity(smart_contract_addr) => smart_contract_addr,
        _ => {
            // This should never happen, as the caller is always an account or a smart contract.
            panic!("Unexpected callee variant: {:?}", context.callee)
        }
    }
}

/// Single ffi function that exposes functionality of the host to wasm clients.
///
/// # Arguments
/// - `ffi_opt`: number which will be interpreted as [FFIMenu]
/// - `input_ptr`: pointer in the wasm execution memory space to the input data.
/// - `input_len`: number of bytes to pass
/// - `cb_alloc`: pointer to function in the wasm code which should be used to allocate in-wasm
///   memory for output data.
/// - `cb_ctx`: If the wasm a-priori knows what will be the size of the output data it can
///   pre-allocate and pass the pointer in this variable. In this case we won't call function under
///   `cb_alloc` to allocate the memory. This can be a cheaper (less gas consuming) option if the
///   wasm creator knows what the memory outpu is.
///
/// # Output:
/// - The return value is either:
///     - Ok(0): The function call was successfull
///     - Ok(err_code): The function call itself was successfull, but there was an error with the
///       input data for the specific host functionality defined by `ffi_opt`
///     - Err(vm_err): There was an error with executing the vm call
#[allow(clippy::too_many_arguments)]
pub fn casper_ffi<S: GlobalStateReader + 'static>(
    mut caller: impl Caller<Context = Context<S>>,
    ffi_opt: u32,
    input_ptr: u32,
    input_len: u32,
    cb_alloc: u32,
    cb_ctx: u32,
) -> VMResult<u32> {
    // get option so we can determine cost, or charge if invalid
    let option: FFIMenu = match TryFrom::try_from(ffi_opt) {
        Ok(option) => option,
        Err(_) => {
            // the following can produce a VMError::OutOfGas error
            let penalty_cost = caller.context().baseline_motes_amount;
            charge_gas(&mut caller, penalty_cost)?;
            return Err(VMError::Execute(ExecuteError::InvalidFFIOption(ffi_opt)));
        }
    };
    if caller.context().sandboxed && !option.allowed_in_sandbox() {
        return Err(VMError::Execute(ExecuteError::AttemptWriteInRestricted));
    }

    let call_cost_definition = match caller.context().ffi_call_costs.get(&ffi_opt) {
        Some(ffi_call_cost) => ffi_call_cost,
        None => return Err(VMError::Fatal(FatalHostError::UnableToValueFFICall)),
    };
    let Some(cost) = call_cost_definition.calculate_gas_cost(input_len as u64) else {
        // Overflowing gas calculation means gas limit was exceeded
        return Err(VMError::OutOfGas);
    };
    let cost = u64::try_from(cost.value()).map_err(|err| {
        error!("Couldn't execute host function due to cost calculation overflow. Details: {err}");
        VMError::Fatal(FatalHostError::TypeConversion)
    })?;

    // the following can produce a VMError::OutOfGas error
    charge_gas(&mut caller, cost)?;

    let input_data = if input_ptr == 0 {
        // If the user didn't pass a input data pointer default to empty data
        Bytes::default()
    } else {
        caller.memory_read(input_ptr, input_len as _)?.into()
    };
    let (output_bytes, exit_code) = match option {
        FFIMenu::Mint(mint_method) => {
            let system_contract_call_opt = SystemContractMenu::Mint(mint_method);
            // Limit the call to remaining gas.
            let gas_limit = caller
                .get_remaining_points()?
                .try_into_remaining()
                .map_err(|_| FatalHostError::TypeConversion)?;

            handle_as_contract_call(&mut caller, system_contract_call_opt, input_data, gas_limit)
        }
        FFIMenu::Auction(auction_method) => {
            let system_contract_call_opt = SystemContractMenu::Auction(auction_method);
            // Limit the call to remaining gas.
            let gas_limit = caller
                .get_remaining_points()?
                .try_into_remaining()
                .map_err(|_| FatalHostError::TypeConversion)?;

            handle_as_contract_call(&mut caller, system_contract_call_opt, input_data, gas_limit)
        }
        FFIMenu::Crypto(crypto_methods) => match crypto_methods {
            CryptoMethods::AltBn128Add => host_alt_bn128_add(input_data),
            CryptoMethods::AltBn128Multiply => host_alt_bn128_mul(input_data),
            CryptoMethods::AltBn128Pairing => host_alt_bn128_pairing(input_data),
            CryptoMethods::GenericHash => host_generic_hash(input_data),
            CryptoMethods::RecoverSecp256K1 => host_recover_secp256k1(input_data),
        },
        FFIMenu::Emit(emit_methods) => match emit_methods {
            EmitMethods::PrintStd => print_std(input_data).map(|code| (None, code)),
            EmitMethods::Native => emit(&mut caller, input_data).map(|code| (None, code)),
        },
        FFIMenu::GlobalState(global_state_methods) => match global_state_methods {
            GlobalStateMethods::Read => host_read(&mut caller, input_data),
            GlobalStateMethods::Write => {
                host_write(&mut caller, input_data).map(|code| (None, code))
            }
            GlobalStateMethods::Remove => {
                host_remove(&mut caller, input_data).map(|code| (None, code))
            }
            GlobalStateMethods::GetBalance => host_env_balance(&mut caller, input_data),
            GlobalStateMethods::GetInfo => host_env_info(&mut caller),
            GlobalStateMethods::Create => host_create(&mut caller, input_data),
        },
        FFIMenu::Control(control_methods) => match control_methods {
            ControlMethods::Call => host_call(&mut caller, input_data),
            ControlMethods::Upgrade => {
                host_upgrade(&mut caller, input_data).map(|code| (None, code))
            }
        },
        FFIMenu::IO(io_methods) => match io_methods {
            IOMethods::Return => host_return(&mut caller, input_data).map(|code| (None, code)),
            IOMethods::CopyInput => host_copy_input(&mut caller),
        },
    }?;

    if let Some(output) = output_bytes {
        let out_ptr: u32 = if cb_alloc != 0 {
            caller.alloc(cb_alloc, output.len(), cb_ctx)?
        } else {
            // treats cb_ctx as data
            cb_ctx
        };
        if out_ptr != 0 {
            caller.memory_write(out_ptr.wrapped_try_into()?, &output)?;
        }
    }
    Ok(exit_code)
}

fn handle_as_contract_call<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    system_call_option: SystemContractMenu,
    input_data: Bytes,
    gas_limit: u64,
) -> VMResult<(Option<Bytes>, u32)> {
    let execute_request = ExecuteRequestBuilder::default()
        .with_initiator(caller.context().initiator)
        .with_caller_key(caller.context().callee)
        .with_gas_limit(gas_limit)
        .with_execution_kind(ExecutionKind::System(system_call_option))
        .with_input(input_data)
        .with_transaction_hash(caller.context().transaction_hash)
        .with_shared_address_generator(Arc::clone(&caller.context().address_generator))
        .with_chain_name(caller.context().chain_name.clone())
        .with_block_time(caller.context().block_time)
        .with_state_hash(Digest::from_raw([0; 32]))
        .with_block_height(1)
        .with_parent_block_hash(BlockHash::new(Digest::from_raw([0; 32])))
        .with_runtime_native_config(caller.context().runtime_native_config.clone())
        .with_authorization_keys(caller.context().authorization_keys.clone())
        .build()
        .map_err(FatalHostError::ExecuteRequestBuildFailure)?;

    exec(caller, execute_request)
}

fn exec<S: GlobalStateReader + 'static>(
    caller: &mut impl Caller<Context = Context<S>>,
    execute_request: ExecuteRequest,
) -> VMResult<(Option<Bytes>, u32)> {
    let tracking_copy = caller.context().tracking_copy.fork2();
    let (gas_usage, host_result, output) =
        match caller.executor().execute(tracking_copy, execute_request) {
            Ok(ExecuteResult {
                host_error,
                output,
                gas_usage,
                effects,
                cache,
                messages,
            }) => {
                let host_result = match host_error {
                    Some(host_error) => Err(host_error),
                    None => {
                        caller
                            .context_mut()
                            .tracking_copy
                            .apply_changes(effects, cache, messages);
                        Ok(())
                    }
                };

                (gas_usage, host_result, output)
            }
            Err(execute_error) => {
                return Err(VMError::Execute(execute_error));
            }
        };

    let gas_spent = gas_usage
        .gas_limit()
        .checked_sub(gas_usage.remaining_points())
        .ok_or(FatalHostError::RemainingGasExceedsGasLimit)?;

    caller.consume_gas(gas_spent)?;

    // this will result in the VM being killed
    if let Err(CallError::Api(api_error)) = host_result {
        return Err(VMError::Execute(ExecuteError::Api(api_error)));
    }

    Ok((output, u32_from_host_result(host_result)))
}
