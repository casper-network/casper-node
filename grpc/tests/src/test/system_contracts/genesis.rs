use once_cell::sync::Lazy;

use casper_engine_test_support::{
    internal::{
        utils, InMemoryWasmTestBuilder, AUCTION_INSTALL_CONTRACT, DEFAULT_AUCTION_DELAY,
        DEFAULT_INITIAL_ERA_ID, DEFAULT_LOCKED_FUNDS_PERIOD, DEFAULT_ROUND_SEIGNIORAGE_RATE,
        DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY, DEFAULT_VALIDATOR_SLOTS,
        DEFAULT_WASM_CONFIG, MINT_INSTALL_CONTRACT, POS_INSTALL_CONTRACT,
        STANDARD_PAYMENT_INSTALL_CONTRACT,
    },
    AccountHash,
};
use casper_execution_engine::{
    core::engine_state::{
        genesis::{ExecConfig, GenesisAccount},
        run_genesis_request::RunGenesisRequest,
        SYSTEM_ACCOUNT_ADDR,
    },
    shared::{motes::Motes, stored_value::StoredValue},
};
use casper_types::{mint::TOTAL_SUPPLY_KEY, ProtocolVersion, PublicKey, SecretKey, U512};

#[cfg(feature = "use-system-contracts")]
const BAD_INSTALL: &str = "standard_payment.wasm";

const GENESIS_CONFIG_HASH: [u8; 32] = [127; 32];
const ACCOUNT_1_BONDED_AMOUNT: u64 = 1_000_000;
const ACCOUNT_2_BONDED_AMOUNT: u64 = 2_000_000;
const ACCOUNT_1_BALANCE: u64 = 1_000_000_000;
const ACCOUNT_2_BALANCE: u64 = 2_000_000_000;

static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([42; SecretKey::ED25519_LENGTH]).into());
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_1_PUBLIC_KEY));
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| SecretKey::ed25519([44; SecretKey::ED25519_LENGTH]).into());
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_2_PUBLIC_KEY));

static GENESIS_CUSTOM_ACCOUNTS: Lazy<Vec<GenesisAccount>> = Lazy::new(|| {
    let account_1 = {
        let account_1_balance = Motes::new(ACCOUNT_1_BALANCE.into());
        let account_1_bonded_amount = Motes::new(ACCOUNT_1_BONDED_AMOUNT.into());
        GenesisAccount::new(
            *ACCOUNT_1_PUBLIC_KEY,
            *ACCOUNT_1_ADDR,
            account_1_balance,
            account_1_bonded_amount,
        )
    };
    let account_2 = {
        let account_2_balance = Motes::new(ACCOUNT_2_BALANCE.into());
        let account_2_bonded_amount = Motes::new(ACCOUNT_2_BONDED_AMOUNT.into());
        GenesisAccount::new(
            *ACCOUNT_2_PUBLIC_KEY,
            *ACCOUNT_2_ADDR,
            account_2_balance,
            account_2_bonded_amount,
        )
    };
    vec![account_1, account_2]
});

#[ignore]
#[test]
fn should_run_genesis() {
    let mint_installer_bytes = utils::read_wasm_file_bytes(MINT_INSTALL_CONTRACT);
    let pos_installer_bytes = utils::read_wasm_file_bytes(POS_INSTALL_CONTRACT);
    let standard_payment_installer_bytes =
        utils::read_wasm_file_bytes(STANDARD_PAYMENT_INSTALL_CONTRACT);
    let auction_installer_bytes = utils::read_wasm_file_bytes(AUCTION_INSTALL_CONTRACT);
    let protocol_version = ProtocolVersion::V1_0_0;
    let wasm_config = *DEFAULT_WASM_CONFIG;
    let system_config = *DEFAULT_SYSTEM_CONFIG;
    let validator_slots = DEFAULT_VALIDATOR_SLOTS;
    let auction_delay = DEFAULT_AUCTION_DELAY;
    let locked_funds_period = DEFAULT_LOCKED_FUNDS_PERIOD;
    let round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE;
    let unbonding_delay = DEFAULT_UNBONDING_DELAY;
    let initial_era_id = DEFAULT_INITIAL_ERA_ID;

    let exec_config = ExecConfig::new(
        mint_installer_bytes,
        pos_installer_bytes,
        standard_payment_installer_bytes,
        auction_installer_bytes,
        GENESIS_CUSTOM_ACCOUNTS.clone(),
        wasm_config,
        system_config,
        validator_slots,
        auction_delay,
        locked_funds_period,
        round_seigniorage_rate,
        unbonding_delay,
        initial_era_id,
    );
    let run_genesis_request =
        RunGenesisRequest::new(GENESIS_CONFIG_HASH.into(), protocol_version, exec_config);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let system_account = builder
        .get_account(SYSTEM_ACCOUNT_ADDR)
        .expect("system account should exist");

    let account_1 = builder
        .get_account(*ACCOUNT_1_ADDR)
        .expect("account 1 should exist");

    let account_2 = builder
        .get_account(*ACCOUNT_2_ADDR)
        .expect("account 2 should exist");

    let system_account_balance_actual = builder.get_purse_balance(system_account.main_purse());
    let account_1_balance_actual = builder.get_purse_balance(account_1.main_purse());
    let account_2_balance_actual = builder.get_purse_balance(account_2.main_purse());

    assert_eq!(system_account_balance_actual, U512::zero());
    assert_eq!(account_1_balance_actual, U512::from(ACCOUNT_1_BALANCE));
    assert_eq!(account_2_balance_actual, U512::from(ACCOUNT_2_BALANCE));

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

#[ignore]
#[test]
fn should_track_total_token_supply_in_mint() {
    let mint_installer_bytes = utils::read_wasm_file_bytes(MINT_INSTALL_CONTRACT);
    let proof_of_stake_installer_bytes = utils::read_wasm_file_bytes(POS_INSTALL_CONTRACT);
    let standard_payment_installer_bytes =
        utils::read_wasm_file_bytes(STANDARD_PAYMENT_INSTALL_CONTRACT);
    let auction_installer_bytes = utils::read_wasm_file_bytes(AUCTION_INSTALL_CONTRACT);
    let accounts = GENESIS_CUSTOM_ACCOUNTS.clone();
    let wasm_config = *DEFAULT_WASM_CONFIG;
    let system_config = *DEFAULT_SYSTEM_CONFIG;
    let protocol_version = ProtocolVersion::V1_0_0;
    let validator_slots = DEFAULT_VALIDATOR_SLOTS;
    let auction_delay = DEFAULT_AUCTION_DELAY;
    let locked_funds_period = DEFAULT_LOCKED_FUNDS_PERIOD;
    let round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE;
    let unbonding_delay = DEFAULT_UNBONDING_DELAY;
    let initial_era_id = DEFAULT_INITIAL_ERA_ID;
    let ee_config = ExecConfig::new(
        mint_installer_bytes,
        proof_of_stake_installer_bytes,
        standard_payment_installer_bytes,
        auction_installer_bytes,
        accounts.clone(),
        wasm_config,
        system_config,
        validator_slots,
        auction_delay,
        locked_funds_period,
        round_seigniorage_rate,
        unbonding_delay,
        initial_era_id,
    );
    let run_genesis_request =
        RunGenesisRequest::new(GENESIS_CONFIG_HASH.into(), protocol_version, ee_config);

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);

    let mint_contract_hash = builder.get_mint_contract_hash();

    let result = builder.query(None, mint_contract_hash.into(), &[TOTAL_SUPPLY_KEY]);

    let total_supply: U512 = if let Ok(StoredValue::CLValue(total_supply)) = result {
        total_supply.into_t().expect("total supply should be U512")
    } else {
        panic!("mint should track total supply");
    };
    let expected_balance: U512 = accounts.iter().map(|item| item.balance().value()).sum();
    let expected_bonded_amount: U512 = accounts
        .iter()
        .map(|item| item.bonded_amount().value())
        .sum();

    // check total supply against expected
    assert_eq!(
        total_supply,
        expected_balance + expected_bonded_amount,
        "unexpected total supply"
    )
}

#[cfg(feature = "use-system-contracts")]
#[ignore]
#[should_panic]
#[test]
fn should_fail_if_bad_mint_install_contract_is_provided() {
    let run_genesis_request = {
        let mint_installer_bytes = utils::read_wasm_file_bytes(BAD_INSTALL);
        let pos_installer_bytes = utils::read_wasm_file_bytes(POS_INSTALL_CONTRACT);
        let standard_payment_installer_bytes =
            utils::read_wasm_file_bytes(STANDARD_PAYMENT_INSTALL_CONTRACT);
        let auction_installer_bytes = utils::read_wasm_file_bytes(AUCTION_INSTALL_CONTRACT);
        let protocol_version = ProtocolVersion::V1_0_0;
        let wasm_config = *DEFAULT_WASM_CONFIG;
        let system_config = *DEFAULT_SYSTEM_CONFIG;
        let validator_slots = DEFAULT_VALIDATOR_SLOTS;
        let auction_delay = DEFAULT_AUCTION_DELAY;
        let locked_funds_period = DEFAULT_LOCKED_FUNDS_PERIOD;
        let round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE;
        let unbonding_delay = DEFAULT_UNBONDING_DELAY;
        let initial_era_id = DEFAULT_INITIAL_ERA_ID;

        let exec_config = ExecConfig::new(
            mint_installer_bytes,
            pos_installer_bytes,
            standard_payment_installer_bytes,
            auction_installer_bytes,
            GENESIS_CUSTOM_ACCOUNTS.clone(),
            wasm_config,
            system_config,
            validator_slots,
            auction_delay,
            locked_funds_period,
            round_seigniorage_rate,
            unbonding_delay,
            initial_era_id,
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
        let mint_installer_bytes = utils::read_wasm_file_bytes(MINT_INSTALL_CONTRACT);
        let pos_installer_bytes = utils::read_wasm_file_bytes(BAD_INSTALL);
        let standard_payment_installer_bytes =
            utils::read_wasm_file_bytes(STANDARD_PAYMENT_INSTALL_CONTRACT);
        let auction_installer_bytes = utils::read_wasm_file_bytes(AUCTION_INSTALL_CONTRACT);
        let protocol_version = ProtocolVersion::V1_0_0;
        let wasm_config = *DEFAULT_WASM_CONFIG;
        let system_config = *DEFAULT_SYSTEM_CONFIG;
        let validator_slots = DEFAULT_VALIDATOR_SLOTS;
        let auction_delay = DEFAULT_AUCTION_DELAY;
        let locked_funds_period = DEFAULT_LOCKED_FUNDS_PERIOD;
        let round_seigniorage_rate = DEFAULT_ROUND_SEIGNIORAGE_RATE;
        let unbonding_delay = DEFAULT_UNBONDING_DELAY;
        let initial_era_id = DEFAULT_INITIAL_ERA_ID;

        let exec_config = ExecConfig::new(
            mint_installer_bytes,
            pos_installer_bytes,
            standard_payment_installer_bytes,
            auction_installer_bytes,
            GENESIS_CUSTOM_ACCOUNTS.clone(),
            wasm_config,
            system_config,
            validator_slots,
            auction_delay,
            locked_funds_period,
            round_seigniorage_rate,
            unbonding_delay,
            initial_era_id,
        );
        RunGenesisRequest::new(GENESIS_CONFIG_HASH.into(), protocol_version, exec_config)
    };

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&run_genesis_request);
}
