#![cfg_attr(target_arch = "wasm32", no_main)]

use casper_contract_sdk::{
    casper_executor_wasm_common::{flags::ReturnFlags, keyspace::Keyspace},
    prelude::*,
    sys::casper_return,
};

const CURRENT_VERSION: &str = "v1";

// This contract is used to assert that calling host functions consumes gas.
// It is by design that it does nothing other than calling appropriate host functions.

// There is no need for these functions to actually do anything meaningful, and it's alright
// if they short-circuit.

#[casper(contract_state)]
pub struct MinHostWrapper;

impl Default for MinHostWrapper {
    fn default() -> Self {
        panic!("Unable to instantiate contract without a constructor");
    }
}

#[casper]
impl MinHostWrapper {
    #[casper(constructor)]
    pub fn new(with_host_fn_call: String) -> Self {
        let ret = Self;
        match with_host_fn_call.as_str() {
            "get_caller" => {
                ret.get_caller();
            }
            "get_block_time" => {
                ret.get_block_time();
            }
            "get_value" => {
                ret.get_transferred_value();
            }
            "get_balance_of" => {
                ret.get_balance_of();
            }
            "call" => {
                ret.call();
            }
            "input" => {
                ret.input();
            }
            "create" => {
                ret.create();
            }
            "print" => {
                ret.print();
            }
            "read" => {
                ret.read();
            }
            "ret" => {
                ret.ret();
            }
            "transfer" => {
                ret.transfer();
            }
            "upgrade" => {
                ret.upgrade();
            }
            "write" => {
                ret.write();
            }
            "write_n_bytes" => {
                ret.write();
            }
            "ret_faulty_flags" => ret.ret_faulty_flags(),
            _ => panic!("Unknown host function"),
        }
        ret
    }

    #[casper(constructor)]
    pub fn new_with_write(byte_count: u64) -> Self {
        let ret = Self;
        ret.write_n_bytes(byte_count);
        ret
    }

    #[casper(constructor)]
    pub fn default() -> Self {
        Self
    }

    pub fn version(&self) -> &str {
        CURRENT_VERSION
    }

    pub fn get_caller(&self) -> Entity {
        casper::get_caller()
    }

    pub fn get_block_time(&self) -> u64 {
        casper::get_block_time()
    }

    pub fn get_transferred_value(&self) -> u64 {
        casper::transferred_value()
    }

    pub fn get_balance_of(&self) -> u64 {
        casper::get_balance_of(&Entity::Account([0u8; 32]))
    }

    pub fn call(&self) {
        casper::casper_call(&[0u8; 32], 0, "", &[]).1.ok();
    }

    pub fn input(&self) {
        casper::copy_input();
    }

    pub fn create(&self) {
        casper::create(None, 0, None, None, None).ok();
    }

    pub fn print(&self) {
        casper::print("");
    }

    pub fn read(&self) {
        casper::read(Keyspace::Context(&[]), |_| None).ok();
    }

    pub fn ret(&self) {
        casper::ret(ReturnFlags::empty(), Some(&[1, 2, 3]));
    }

    pub fn transfer(&self) {
        casper::transfer(&[0; 32], 0).ok();
    }

    pub fn upgrade(&self) {
        casper::upgrade(&[], None, None).ok();
    }

    pub fn write(&self) {
        casper::write(Keyspace::Context(&[]), &[]).ok();
    }

    pub fn write_n_bytes(&self, n: u64) {
        let buffer = vec![0; n as usize];
        casper::write(Keyspace::Context(&[0]), &buffer).ok();
    }

    pub fn ret_faulty_flags(&self) {
        let all_flags_bits = ReturnFlags::all().bits();
        let faulty_flags = all_flags_bits << 1;
        if faulty_flags == all_flags_bits {
            // By pure coincidence all the current flags of ReturnFlags are homomorphic when
            // shifted by one byte. If this happens we need to produce a different
            // "faulty_flags value"
            casper::ret(ReturnFlags::empty(), Some(&[1, 2, 3]));
        }
        let data = [1, 2, 3];
        let (data_ptr, data_len) = ((&data).as_ptr(), (&data).len());
        unsafe { casper_return(faulty_flags, data_ptr, data_len) };
    }
}
