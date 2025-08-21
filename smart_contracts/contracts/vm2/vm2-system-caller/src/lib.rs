#![cfg_attr(target_family = "wasm", no_main)]

pub mod exports {
    use casper_contract_sdk::casper_executor_wasm_common::flags::ReturnFlags;
    use casper_contract_sdk::{
        casper::casper_system,
        casper::ret,
        prelude::*,
        types::{PublicKey, SystemContractOption},
    };

    #[casper(export)]
    pub fn call(opt: u32) {
        use borsh;

        match SystemContractOption::try_from(opt) {
            Ok(option) => {
                match option {
                    SystemContractOption::Transfer => {}
                    SystemContractOption::Burn => ret(ReturnFlags::REVERT, None),
                    SystemContractOption::ActivateBid => {
                        let input = borsh::to_vec(&(PublicKey::Ed25519([1; 32]),))
                            .expect("Serialization to succeed");
                        let (_output, _result) =
                            casper_system(SystemContractOption::ActivateBid.into(), &input);
                        return;
                    }
                    SystemContractOption::Bid => {}
                    SystemContractOption::Withdraw => {}
                    SystemContractOption::Delegate => {}
                    SystemContractOption::Undelegate => {}
                    SystemContractOption::Redelegate => {}
                    SystemContractOption::AddReservation => {}
                    SystemContractOption::CancelReservation => {}
                    SystemContractOption::ChangePublicKey => {}
                };
            }
            Err(_) => match &borsh::to_vec(&(opt,)) {
                Ok(bytes) => ret(ReturnFlags::REVERT, Some(bytes)),
                Err(_) => unreachable!("failed to serialize opt"),
            },
        }
    }
}
