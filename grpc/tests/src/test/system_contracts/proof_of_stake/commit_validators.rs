use num_traits::Zero;
use std::collections::HashMap;

use casperlabs_engine_test_support::{
    internal::{utils, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS},
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_node::{
    crypto::asymmetric_key::{PublicKey, SecretKey},
    types::Motes,
    GenesisAccount,
};
use casperlabs_types::{account::AccountHash, RuntimeArgs, U512};

const CONTRACT_LOCAL_STATE: &str = "do_nothing.wasm";
const ACCOUNT_1_SECRET_KEY_BYTES: [u8; 32] = [1u8; 32];
const ACCOUNT_1_BALANCE: u64 = 2000;
const ACCOUNT_1_BOND: u64 = 1000;

const ACCOUNT_2_SECRET_KEY_BYTES: [u8; 32] = [2u8; 32];
const ACCOUNT_2_BALANCE: u64 = 2000;
const ACCOUNT_2_BOND: u64 = 200;

#[ignore]
#[test]
fn should_return_bonded_validators() {
    let account_1_secret_key = SecretKey::new_ed25519(ACCOUNT_1_SECRET_KEY_BYTES);
    let account_1_public_key = PublicKey::from(&account_1_secret_key);

    let account_2_secret_key = SecretKey::new_ed25519(ACCOUNT_2_SECRET_KEY_BYTES);
    let account_2_public_key = PublicKey::from(&account_2_secret_key);

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account_1 = GenesisAccount::with_public_key(
            account_1_public_key,
            Motes::new(ACCOUNT_1_BALANCE.into()),
            Motes::new(ACCOUNT_1_BOND.into()),
        );
        let account_2 = GenesisAccount::with_public_key(
            account_2_public_key,
            Motes::new(ACCOUNT_2_BALANCE.into()),
            Motes::new(ACCOUNT_2_BOND.into()),
        );
        tmp.push(account_1);
        tmp.push(account_2);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts.clone());

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_LOCAL_STATE,
        RuntimeArgs::default(),
    )
    .build();

    let actual = InMemoryWasmTestBuilder::default()
        .run_genesis(&run_genesis_request)
        .exec(exec_request)
        .commit()
        .get_bonded_validators()[0]
        .clone();

    let expected: HashMap<AccountHash, U512> = {
        let zero = Motes::zero();
        accounts
            .iter()
            .filter_map(move |genesis_account| {
                if genesis_account.bonded_amount() > zero {
                    Some((
                        genesis_account.public_key().to_account_hash(),
                        genesis_account.bonded_amount().value(),
                    ))
                } else {
                    None
                }
            })
            .collect()
    };

    assert_eq!(actual, expected);
}
