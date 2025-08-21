#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::{
        casper::{casper_system, ret},
        casper_executor_wasm_common::flags::ReturnFlags,
        prelude::*,
        types::{PublicKey, SystemContractOption},
    };

    #[casper(export)]
    pub fn call(opt: u32) {
        use borsh;

        let option = match SystemContractOption::try_from(opt) {
            Ok(option) => option,
            Err(_) => match &borsh::to_vec(&(opt,)) {
                Ok(bytes) => return ret(ReturnFlags::REVERT, Some(bytes)),
                Err(_) => unreachable!("failed to serialize opt"),
            },
        };
        let input = match option {
            SystemContractOption::Transfer => None,
            SystemContractOption::Burn => None,
            SystemContractOption::ActivateBid => {
                let input = borsh::to_vec(&(PublicKey::Ed25519([1; 32]),))
                    .expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::Bid => {
                let input = borsh::to_vec(&(
                    PublicKey::Ed25519([1; 32]),
                    9u8,
                    88u64,
                    0u64,
                    99999u64,
                    0u32,
                ))
                .expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::Withdraw => {
                let input = borsh::to_vec(&(PublicKey::Ed25519([1; 32]), 1u64))
                    .expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::Delegate => None,
            SystemContractOption::Undelegate => None,
            SystemContractOption::Redelegate => None,
            SystemContractOption::AddReservation => None,
            SystemContractOption::CancelReservation => None,
            SystemContractOption::ChangePublicKey => {
                let args = (PublicKey::Ed25519([1; 32]), PublicKey::Ed25519([255; 32]));
                let input = borsh::to_vec(&args).expect("Serialization to succeed");
                Some(input)
            }
        };

        match &input {
            Some(input) => {
                let (_output, _result) = casper_system(option.into(), &input);
            }
            None => ret(ReturnFlags::REVERT, None),
        }
    }
}
