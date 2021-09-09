#![no_std]
// #![cfg_attr(target_arch = "wasm32", no_main)]
#![no_main]

extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec,
};

use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_erc20::{
    constants::{ARG_ADDRESS, ARG_AMOUNT, ARG_OWNER},
    error::Error,
    Address, ERC20,
};
use casper_types::{
    account::AccountHash, CLType, CLTyped, CLValue, ContractHash, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, Parameter, U256,
};

const METHOD_MINT: &str = "mint";
const METHOD_BURN: &str = "burn";

/// "erc20" is not mentioned here intentionally as the functionality is not compatible with ERC20
/// token standard.
const TEST_CONTRACT_KEY: &str = "test_contract";
const TOKEN_NAME: &str = "CasperTest";
const TOKEN_SYMBOL: &str = "CSPRT";
const TOKEN_DECIMALS: u8 = 100;
const TOKEN_TOTAL_SUPPLY: u64 = 1_000_000_000;

const TOKEN_OWNER_ADDRESS_1: Address = Address::Account(AccountHash::new([42; 32]));
const TOKEN_OWNER_AMOUNT_1: u64 = 1_000_000;
const TOKEN_OWNER_ADDRESS_2: Address = Address::Contract(ContractHash::new([42; 32]));
const TOKEN_OWNER_AMOUNT_2: u64 = 2_000_000;

#[derive(Default)]
struct TestToken {
    erc20: ERC20,
}

impl TestToken {
    pub fn install() -> Result<TestToken, Error> {
        let name: String = TOKEN_NAME.to_string();
        let symbol: String = TOKEN_SYMBOL.to_string();
        let decimals = TOKEN_DECIMALS;
        let total_supply = U256::from(TOKEN_TOTAL_SUPPLY);

        let mut entry_points = EntryPoints::new();

        let mint_entrypoint = EntryPoint::new(
            METHOD_MINT,
            vec![
                Parameter::new(ARG_OWNER, Address::cl_type()),
                Parameter::new(ARG_AMOUNT, U256::cl_type()),
            ],
            CLType::Unit,
            // NOTE: For security reasons never use this entrypoint definition in a production
            // contract. This is marks the entry point as public.
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        let burn_entrypoint = EntryPoint::new(
            METHOD_BURN,
            vec![
                Parameter::new(ARG_OWNER, Address::cl_type()),
                Parameter::new(ARG_AMOUNT, U256::cl_type()),
            ],
            CLType::Unit,
            // NOTE: For security reasons never use this entrypoint definition in a production
            // contract. This is marks the entry point as public.
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(casper_erc20::entry_points::get_balance_of_entry_point());
        entry_points.add_entry_point(mint_entrypoint);
        entry_points.add_entry_point(burn_entrypoint);

        // Caution: This test uses `install_custom` without providing default entrypoints as
        // described by ERC20 token standard.
        //
        // This is unsafe and this test contract is not a ERC20 token standard-compliant token.
        // Contract developers should use examples/erc20 contract instead as a template for writing
        // their own tokens.
        let erc20 = ERC20::install_custom(
            name,
            symbol,
            decimals,
            total_supply,
            TEST_CONTRACT_KEY,
            entry_points,
        )?;
        Ok(TestToken { erc20 })
    }
}

impl core::ops::Deref for TestToken {
    type Target = ERC20;

    fn deref(&self) -> &Self::Target {
        &self.erc20
    }
}

impl core::ops::DerefMut for TestToken {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.erc20
    }
}

#[no_mangle]
pub extern "C" fn balance_of() {
    let address: Address = runtime::get_named_arg(ARG_ADDRESS);
    let val = TestToken::default().balance_of(&address);
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn mint() {
    let owner: Address = runtime::get_named_arg(ARG_OWNER);
    let amount: U256 = runtime::get_named_arg(ARG_AMOUNT);
    TestToken::default().mint(&owner, amount).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn burn() {
    let owner: Address = runtime::get_named_arg(ARG_OWNER);
    let amount: U256 = runtime::get_named_arg(ARG_AMOUNT);
    TestToken::default().burn(&owner, amount).unwrap_or_revert();
}

#[no_mangle]
fn call() {
    let mut test_token = TestToken::install().unwrap_or_revert();

    test_token
        .mint(&TOKEN_OWNER_ADDRESS_1, U256::from(TOKEN_OWNER_AMOUNT_1))
        .unwrap_or_revert();

    test_token
        .mint(&TOKEN_OWNER_ADDRESS_2, U256::from(TOKEN_OWNER_AMOUNT_2))
        .unwrap_or_revert();
}
