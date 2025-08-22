#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::{
        casper::{casper_system, ret},
        casper_executor_wasm_common::flags::ReturnFlags,
        prelude::*,
        types::{DelegatorKind, PublicKey, Reservation, SystemContractOption},
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
                    2u32,
                ))
                .expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::Withdraw => {
                let input = borsh::to_vec(&(PublicKey::Ed25519([1; 32]), 1u64))
                    .expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::Delegate => {
                let kind = DelegatorKind::PublicKey(PublicKey::Ed25519([255; 32]));
                let validator = PublicKey::Ed25519([1; 32]);
                let amount = 10u64;
                let input =
                    borsh::to_vec(&(kind, validator, amount)).expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::Undelegate => None,
            SystemContractOption::Redelegate => None,
            SystemContractOption::AddReservation => {
                let pub_k = PublicKey::Ed25519([1; 32]);
                let res_pu = Reservation::new(DelegatorKind::Purse([254; 32]), pub_k, 1);
                let res_pk = Reservation::new(
                    DelegatorKind::PublicKey(PublicKey::Ed25519([255; 32])),
                    pub_k,
                    1,
                );
                let reservations = vec![res_pu, res_pk];
                let args = (reservations,);
                let input = borsh::to_vec(&args).expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::CancelReservation => {
                let reservations = vec![
                    DelegatorKind::Purse([254; 32]),
                    DelegatorKind::PublicKey(PublicKey::Ed25519([255; 32])),
                ];
                let args = (PublicKey::Ed25519([1; 32]), reservations);
                let input = borsh::to_vec(&args).expect("Serialization to succeed");
                Some(input)
            }
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
