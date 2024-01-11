#![no_std]
#![no_main]

extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec,
};

use casper_contract::contract_api::{runtime, storage};
use casper_types::{EraId, Key};

#[no_mangle]
pub extern "C" fn call() {
    let fn_arg: String = runtime::get_named_arg("fn");
    match fn_arg.as_str() {
        "write" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "read" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "add" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "new" => {
            for _i in 0..u64::MAX {
                let _n = storage::new_uref(()); // 0:03
            }
        }
        "ret" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "call_contract" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_key" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "has_key" => {
            let exists: bool = runtime::get_named_arg("exists");
            if exists {
                runtime::put_key("k", Key::EraInfo(EraId::new(0)));
            }
            for _i in 0..u64::MAX {
                let _b = runtime::has_key("k");
            }
        }
        "put_key" => {
            let large: bool = runtime::get_named_arg("large");
            let key = if large {
                let uref = storage::new_uref(());
                Key::URef(uref)
            } else {
                Key::EraInfo(EraId::new(0))
            };
            for i in 0..u64::MAX {
                runtime::put_key(&i.to_string(), key); // 11:25
            }
        }
        "is_valid_uref" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "add_associated_key" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "remove_associated_key" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "update_associated_key" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "set_action_threshold" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "load_named_keys" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "remove_key" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_caller" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_blocktime" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "create_purse" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "transfer_to_account" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "transfer_from_purse_to_account" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "transfer_from_purse_to_purse" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_balance" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_phase" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_systemcontract" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_main_purse" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "read_host_buffer" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "create_contract_package_at_hash" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "add_contract_version" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "disable_contract_version" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "call_versioned_contract" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "create_contract_user_group" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "print" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_runtime_arg_size" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_runtime_arg" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "remove_contract_user_group" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "extend_contract_user_group_urefs" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "remove_contract_user_group_urefs" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "blake2b" => {
            let len: u32 = runtime::get_named_arg("len");
            let data = vec![1; len as usize];
            // 1_000_000 -> 72:00,  10_000 -> 1:28,  1 -> 0:07
            for _i in 0..u64::MAX {
                let _hash = runtime::blake2b(&data);
            }
        }
        "new_dictionary" => {
            for i in 0..u64::MAX {
                let _uref = storage::new_dictionary(&i.to_string()).unwrap(); // 1:40
            }
        }
        "dictionary_get" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "dictionary_put" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "load_call_stack" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "load_authorization_keys" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "random_bytes" => {
            for _i in 0..u64::MAX {
                let _n = runtime::random_bytes(); // 0:05
            }
        }
        "dictionary_read" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "enable_contract_version" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        _ => panic!(),
    }
}
