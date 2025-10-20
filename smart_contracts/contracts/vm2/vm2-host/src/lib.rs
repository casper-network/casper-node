#![cfg_attr(target_arch = "wasm32", no_main)]

use casper_contract_sdk::{
    casper::casper_ffi,
    casper_executor_wasm_common::{flags::ReturnFlags, keyspace::Keyspace},
    prelude::*,
    serializers::borsh,
    types::{EntityAddr, HashAlgorithm, IOFunctionOption},
};

const CURRENT_VERSION: &str = "v1";

// This contract is used to assert that calling host functions consumes gas and doesn't panic.

// There is no need for these functions to actually do anything meaningful, but the execution
// should be succesful.

#[casper(contract_state)]
pub struct MinimalHostWrapper;

impl Default for MinimalHostWrapper {
    fn default() -> Self {
        panic!("Unable to instantiate contract without a constructor");
    }
}

#[casper]
#[allow(clippy::should_implement_trait)]
impl MinimalHostWrapper {
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
            "generic_hash" => ret.generic_hash(),
            "recover_secp256k1" => ret.recover_secp256k1(),
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

    pub fn call_add_bid(&self) {
        // public key must match initiator's account hash
        casper::casper_call(&[0u8; 32], 0, "", &[]).1.ok();
    }

    pub fn call_delegate(&self) {
        // should be able to send delegate from purse?
        casper::casper_call(&[0u8; 32], 0, "", &[]).1.ok();
    }

    pub fn input(&self) {
        casper::copy_input();
    }

    pub fn create(&self) {
        casper::create(None, 0, None, None, None).ok();
    }

    pub fn print(&self) {
        let _ = casper::print("");
    }

    pub fn read(&self) {
        casper::read(Keyspace::Context(&[]), |_| None).ok();
    }

    pub fn ret(&self) {
        casper::ret(ReturnFlags::empty(), Some(&[1, 2, 3]));
    }

    pub fn transfer(&self) {
        casper::transfer(&EntityAddr::SmartContract([0; 32]), 0).ok();
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
        let data: [u8; 3] = [1, 2, 3];
        let args = (faulty_flags, Some(Vec::from(data)));
        let arg_bytes = borsh::to_vec(&args).expect("Expected borsh to work");
        let _ = casper_ffi(IOFunctionOption::Return.into(), &arg_bytes);
    }

    pub fn generic_hash(&self) {
        let data = [1, 1, 2, 5, 14, 42, 132];

        assert_eq!(
            casper::generic_hash(&data, HashAlgorithm::Blake2b),
            Ok([
                101, 134, 221, 117, 175, 165, 62, 143, 176, 114, 113, 246, 5, 183, 189, 207, 11,
                104, 170, 199, 146, 141, 122, 205, 157, 158, 233, 5, 125, 81, 23, 241
            ]),
        );

        assert_eq!(
            casper::generic_hash(&data, HashAlgorithm::Blake3),
            Ok([
                126, 230, 212, 24, 35, 87, 8, 3, 4, 62, 160, 20, 182, 106, 115, 229, 187, 7, 147,
                32, 244, 103, 58, 70, 70, 67, 7, 151, 246, 32, 38, 93
            ]),
        );

        assert_eq!(
            casper::generic_hash(&data, HashAlgorithm::Sha256),
            Ok([
                0, 230, 115, 1, 88, 98, 21, 212, 204, 82, 181, 141, 113, 17, 93, 117, 110, 170, 80,
                53, 20, 125, 106, 121, 92, 98, 75, 159, 117, 104, 172, 57
            ]),
        );

        assert_eq!(
            casper::generic_hash(&data, HashAlgorithm::Keccak256),
            Ok([
                114, 172, 78, 22, 211, 115, 239, 44, 244, 233, 234, 252, 93, 139, 253, 67, 225, 90,
                77, 165, 66, 13, 132, 134, 234, 199, 38, 235, 176, 138, 236, 105
            ]),
        );
    }

    pub fn recover_secp256k1(&self) {
        casper::recover_secp256k1(&[0], &[0], 1).ok();
    }
}
