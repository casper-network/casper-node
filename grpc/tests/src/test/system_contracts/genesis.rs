use engine_test_support::internal::{
    utils, InMemoryWasmTestBuilder, DEFAULT_WASM_COSTS, MINT_INSTALL_CONTRACT,
    POS_INSTALL_CONTRACT, STANDARD_PAYMENT_INSTALL_CONTRACT,
};
use node::contract_core::engine_state::{
    genesis::{ExecConfig, GenesisAccount},
    run_genesis_request::RunGenesisRequest,
    SYSTEM_ACCOUNT_ADDR,
};
use node::contract_shared::{motes::Motes, stored_value::StoredValue};
use types::{account::AccountHash, ProtocolVersion, U512};

#[cfg(feature = "use-system-contracts")]
const BAD_INSTALL: &str = "standard_payment.wasm";

const GENESIS_CONFIG_HASH: [u8; 32] = [127; 32];
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const ACCOUNT_2_ADDR: AccountHash = AccountHash::new([2u8; 32]);
const ACCOUNT_1_BONDED_AMOUNT: u64 = 1_000_000;
const ACCOUNT_2_BONDED_AMOUNT: u64 = 2_000_000;
const ACCOUNT_1_BALANCE: u64 = 1_000_000_000;
const ACCOUNT_2_BALANCE: u64 = 2_000_000_000;

#[ignore]
#[test]
fn should_run_genesis() {
    let account_1_balance = Motes::new(ACCOUNT_1_BALANCE.into());
    let account_1 = {
        let account_1_account_hash = ACCOUNT_1_ADDR;
        let account_1_bonded_amount = Motes::new(ACCOUNT_1_BONDED_AMOUNT.into());
        GenesisAccount::new(
            account_1_account_hash,
            account_1_balance,
            account_1_bonded_amount,
        )
    };

    let account_2_balance = Motes::new(ACCOUNT_2_BALANCE.into());
    let account_2 = {
        let account_2_account_hash = ACCOUNT_2_ADDR;
        let account_2_bonded_amount = Motes::new(ACCOUNT_2_BONDED_AMOUNT.into());
        GenesisAccount::new(
            account_2_account_hash,
            account_2_balance,
            account_2_bonded_amount,
        )
    };

    let mint_installer_bytes = utils::read_wasm_file_bytes(MINT_INSTALL_CONTRACT);
    let pos_installer_bytes = utils::read_wasm_file_bytes(POS_INSTALL_CONTRACT);
    let standard_payment_installer_bytes =
        utils::read_wasm_file_bytes(STANDARD_PAYMENT_INSTALL_CONTRACT);
    let accounts = vec![account_1, account_2];
    let protocol_version = ProtocolVersion::V1_0_0;
    let wasm_costs = *DEFAULT_WASM_COSTS;

    let exec_config = ExecConfig::new(
        mint_installer_bytes,
        pos_installer_bytes,
        standard_payment_installer_bytes,
        accounts,
        wasm_costs,
    );
    let run_genesis_request =
        RunGenesisRequest::new(GENESIS_CONFIG_HASH.into(), protocol_version, exec_config);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let system_account = builder
        .get_account(SYSTEM_ACCOUNT_ADDR)
        .expect("system account should exist");

    let account_1 = builder
        .get_account(ACCOUNT_1_ADDR)
        .expect("account 1 should exist");

    let account_2 = builder
        .get_account(ACCOUNT_2_ADDR)
        .expect("account 2 should exist");

    let system_account_balance_actual = builder.get_purse_balance(system_account.main_purse());
    let account_1_balance_actual = builder.get_purse_balance(account_1.main_purse());
    let account_2_balance_actual = builder.get_purse_balance(account_2.main_purse());

    assert_eq!(system_account_balance_actual, U512::zero());
    assert_eq!(account_1_balance_actual, account_1_balance.value());
    assert_eq!(account_2_balance_actual, account_2_balance.value());

    let mint_contract_hash = builder.get_mint_contract_hash();
    let pos_contract_hash = builder.get_pos_contract_hash();

    let result = builder.query(None, mint_contract_hash.into(), &[]);
    if let Ok(StoredValue::Contract(_)) = result {
        // Contract exists at mint contract hash
    } else {
        panic!("contract not found at mint hash");
    }

    if let Ok(StoredValue::Contract(_)) = builder.query(None, pos_contract_hash.into(), &[]) {
        // Contract exists at pos contract hash
    } else {
        panic!("contract not found at pos hash");
    }
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[should_panic]
#[test]
fn should_fail_if_bad_mint_install_contract_is_provided() {
    let run_genesis_request = {
        let account_1 = {
            let account_1_account_hash = ACCOUNT_1_ADDR;
            let account_1_balance = Motes::new(ACCOUNT_1_BALANCE.into());
            let account_1_bonded_amount = Motes::new(ACCOUNT_1_BONDED_AMOUNT.into());
            GenesisAccount::new(
                account_1_account_hash,
                account_1_balance,
                account_1_bonded_amount,
            )
        };
        let account_2 = {
            let account_2_account_hash = ACCOUNT_2_ADDR;
            let account_2_balance = Motes::new(ACCOUNT_2_BALANCE.into());
            let account_2_bonded_amount = Motes::new(ACCOUNT_2_BONDED_AMOUNT.into());
            GenesisAccount::new(
                account_2_account_hash,
                account_2_balance,
                account_2_bonded_amount,
            )
        };
        let mint_installer_bytes = utils::read_wasm_file_bytes(BAD_INSTALL);
        let pos_installer_bytes = utils::read_wasm_file_bytes(POS_INSTALL_CONTRACT);
        let standard_payment_installer_bytes =
            utils::read_wasm_file_bytes(STANDARD_PAYMENT_INSTALL_CONTRACT);
        let accounts = vec![account_1, account_2];
        let protocol_version = ProtocolVersion::V1_0_0;
        let wasm_costs = *DEFAULT_WASM_COSTS;

        let exec_config = ExecConfig::new(
            mint_installer_bytes,
            pos_installer_bytes,
            standard_payment_installer_bytes,
            accounts,
            wasm_costs,
        );
        RunGenesisRequest::new(GENESIS_CONFIG_HASH.into(), protocol_version, exec_config)
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[should_panic]
#[test]
fn should_fail_if_bad_pos_install_contract_is_provided() {
    let run_genesis_request = {
        let account_1 = {
            let account_1_account_hash = ACCOUNT_1_ADDR;
            let account_1_balance = Motes::new(ACCOUNT_1_BALANCE.into());
            let account_1_bonded_amount = Motes::new(ACCOUNT_1_BONDED_AMOUNT.into());
            GenesisAccount::new(
                account_1_account_hash,
                account_1_balance,
                account_1_bonded_amount,
            )
        };
        let account_2 = {
            let account_2_account_hash = ACCOUNT_2_ADDR;
            let account_2_balance = Motes::new(ACCOUNT_2_BALANCE.into());
            let account_2_bonded_amount = Motes::new(ACCOUNT_2_BONDED_AMOUNT.into());
            GenesisAccount::new(
                account_2_account_hash,
                account_2_balance,
                account_2_bonded_amount,
            )
        };
        let mint_installer_bytes = utils::read_wasm_file_bytes(MINT_INSTALL_CONTRACT);
        let pos_installer_bytes = utils::read_wasm_file_bytes(BAD_INSTALL);
        let standard_payment_installer_bytes =
            utils::read_wasm_file_bytes(STANDARD_PAYMENT_INSTALL_CONTRACT);
        let accounts = vec![account_1, account_2];
        let protocol_version = ProtocolVersion::V1_0_0;
        let wasm_costs = *DEFAULT_WASM_COSTS;
        let exec_config = ExecConfig::new(
            mint_installer_bytes,
            pos_installer_bytes,
            standard_payment_installer_bytes,
            accounts,
            wasm_costs,
        );
        RunGenesisRequest::new(GENESIS_CONFIG_HASH.into(), protocol_version, exec_config)
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);
}
