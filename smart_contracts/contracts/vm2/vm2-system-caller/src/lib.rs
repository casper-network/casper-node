#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::{
        casper::{casper_ffi, ret},
        casper_executor_wasm_common::flags::ReturnFlags,
        prelude::*,
        types::{DelegatorKind, EntityAddr, PublicKey, Reservation, SystemContractOption},
    };

    #[casper(export)]
    pub fn call(opt: u32, purse_delegation: bool) {
        use borsh;

        let option = match SystemContractOption::try_from(opt) {
            Ok(option) => option,
            Err(_) => match &borsh::to_vec(&(opt,)) {
                Ok(bytes) => return ret(ReturnFlags::ROLLBACK, Some(bytes)),
                Err(_) => unreachable!("failed to serialize opt"),
            },
        };
        let input = match option {
            SystemContractOption::Transfer => {
                let entity_addr = EntityAddr::Account([
                    158, 17, 242, 57, 55, 151, 207, 10, 36, 74, 126, 15, 148, 172, 106, 131, 189,
                    124, 170, 34, 9, 239, 243, 182, 232, 2, 20, 162, 136, 218, 113, 238,
                ]);
                let input =
                    borsh::to_vec(&(entity_addr, 100u64)).expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::TransferPurse => {
                let uref_addr: casper_contract_sdk::types::Address = [
                    134, 139, 79, 97, 16, 65, 173, 186, 126, 93, 235, 111, 229, 224, 29, 144, 186,
                    66, 74, 244, 236, 214, 63, 64, 207, 67, 100, 16, 45, 199, 96, 170,
                ];
                let input = borsh::to_vec(&(uref_addr, 100u64)).expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::Burn => {
                let input = borsh::to_vec(&(100u64,)).expect("Serialization to succeed");
                Some(input)
            }
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
            SystemContractOption::Undelegate => {
                let kind = DelegatorKind::PublicKey(PublicKey::Ed25519([255; 32]));
                let validator = PublicKey::Ed25519([1; 32]);
                let amount = 10u64;
                let input =
                    borsh::to_vec(&(kind, validator, amount)).expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::Redelegate => {
                let kind = DelegatorKind::PublicKey(PublicKey::Ed25519([255; 32]));
                let validator = PublicKey::Ed25519([1; 32]);
                let amount = 10u64;
                // though there is no good reason to do so,
                // it is allowed to redelegate back to the original validator,
                // so doing that here instead of dealing w set up for a 2nd validator
                let input = borsh::to_vec(&(kind, validator, amount, validator))
                    .expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::AddReservation => {
                let pub_k = PublicKey::Ed25519([1; 32]);
                let reservation = if purse_delegation {
                    Reservation::new(DelegatorKind::Purse([254; 32]), pub_k, 1)
                } else {
                    Reservation::new(
                        DelegatorKind::PublicKey(PublicKey::Ed25519([255; 32])),
                        pub_k,
                        1,
                    )
                };

                let args = (reservation,);
                let input = borsh::to_vec(&args).expect("Serialization to succeed");
                Some(input)
            }
            SystemContractOption::CancelReservation => {
                let delegator_kind = if purse_delegation {
                    DelegatorKind::Purse([254; 32])
                } else {
                    DelegatorKind::PublicKey(PublicKey::Ed25519([255; 32]))
                };
                let args = (PublicKey::Ed25519([1; 32]), delegator_kind);
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
                let (_output, _result) = casper_ffi(option.into(), &input);
            }
            None => ret(ReturnFlags::ROLLBACK, None),
        }
    }
}
