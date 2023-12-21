#![cfg_attr(target_arch = "wasm32", no_main)]
pub mod error;

use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::{casper, CasperABI, Contract, CasperSchema};
use casper_sdk::{collections::Map, host, revert, schema::CasperSchema, types::Address, Contract};
use error::Cep18Error;
use std::string::String;

#[derive(Contract, CasperSchema, BorshSerialize, BorshDeserialize, CasperABI, Debug)]
struct CEP18 {
    name: String,
    symbol: String,
    decimals: u8,
    total_supply: u64, // TODO: U256
    balances: Map<Address, u64>,
    allowances: Map<(Address, Address), u64>,
}

impl Default for CEP18 {
    fn default() -> Self {
        Self {
            name: "Default name".to_string(),
            symbol: "Default symbol".to_string(),
            decimals: 0,
            total_supply: 0,
            balances: Map::new("balances"),
            allowances: Map::new("allowances"),
        }
    }
}

#[casper(entry_points)]
impl CEP18 {
    #[constructor]
    pub fn new() -> Self {
        let mut instance = Self::default();
        instance.balances.insert(&[2; 32], &1);
        instance
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    pub fn total_supply(&self) -> u64 {
        self.total_supply
    }

    pub fn balance_of(&self, address: Address) -> u64 {
        self.balances.get(&address).unwrap_or_default()
    }

    fn allowance(&self, spender: Address, owner: Address) {
        self.allowances.get(&(spender, owner)).unwrap_or_default();
    }

    fn approve(&mut self, spender: Address, amount: u64) -> Result<(), Cep18Error> {
        let owner = host::get_caller();
        if owner == spender {
            return revert!(Err(Cep18Error::CannotTargetSelfUser));
        }
        let lookup_key = (owner, spender);
        self.allowances.insert(&lookup_key, &amount);
        Ok(())
    }

    // fn decrease_allowance(&self, spender: Address, amount: u64) -> Result<(), Cep18Error> {
    //     // let owner = utils::get_immediate_caller_address().unwrap_or_revert();
    //     // let spender: Key = runtime::get_named_arg(SPENDER);
    //     // if spender == owner {
    //     //     revert(Cep18Error::CannotTargetSelfUser);
    //     // }
    //     // let amount: U256 = runtime::get_named_arg(AMOUNT);
    //     // let allowances_uref = get_allowances_uref();
    //     // let current_allowance = read_allowance_from(allowances_uref, owner, spender);
    //     // let new_allowance = current_allowance.saturating_sub(amount);
    //     // write_allowance_to(allowances_uref, owner, spender, new_allowance);
    //     // events::record_event_dictionary(Event::DecreaseAllowance(DecreaseAllowance {
    //     //     owner,
    //     //     spender,
    //     //     decr_by: amount,
    //     //     allowance: new_allowance,
    //     // }))
    //     todo!()
    // }

    // fn increase_allowance(&self, spender: Address, amount: u64) -> Result<(), Cep18Error> {
    //     todo!()
    //     // let owner = utils::get_immediate_caller_address().unwrap_or_revert();
    //     // let spender: Key = runtime::get_named_arg(SPENDER);
    //     // if spender == owner {
    //     //     revert(Cep18Error::CannotTargetSelfUser);
    //     // }
    //     // let amount: U256 = runtime::get_named_arg(AMOUNT);
    //     // let allowances_uref = get_allowances_uref();
    //     // let current_allowance = read_allowance_from(allowances_uref, owner, spender);
    //     // let new_allowance = current_allowance.saturating_add(amount);
    //     // write_allowance_to(allowances_uref, owner, spender, new_allowance);
    //     // events::record_event_dictionary(Event::IncreaseAllowance(IncreaseAllowance {
    //     //     owner,
    //     //     spender,
    //     //     allowance: new_allowance,
    //     //     inc_by: amount,
    //     // }))
    // }

    // fn transfer(&self, recipient: Address, amount: u64) -> Result<(), Cep18Error> {
    //     todo!()
    //     // let sender = utils::get_immediate_caller_address().unwrap_or_revert();
    //     // let recipient: Key = runtime::get_named_arg(RECIPIENT);
    //     // if sender == recipient {
    //     //     revert(Cep18Error::CannotTargetSelfUser);
    //     // }
    //     // let amount: U256 = runtime::get_named_arg(AMOUNT);

    //     // transfer_balance(sender, recipient, amount).unwrap_or_revert();
    //     // events::record_event_dictionary(Event::Transfer(Transfer {
    //     //     sender,
    //     //     recipient,
    //     //     amount,
    //     // }))
    // }

    // fn transfer_from(
    //     &self,
    //     owner: Address,
    //     recipient: Address,
    //     amount: u64,
    // ) -> Result<(), Cep18Error> {
    //     todo!()
    //     // let spender = utils::get_immediate_caller_address().unwrap_or_revert();
    //     // let recipient: Key = runtime::get_named_arg(RECIPIENT);
    //     // let owner: Key = runtime::get_named_arg(OWNER);
    //     // if owner == recipient {
    //     //     revert(Cep18Error::CannotTargetSelfUser);
    //     // }
    //     // let amount: U256 = runtime::get_named_arg(AMOUNT);
    //     // if amount.is_zero() {
    //     //     return;
    //     // }

    //     // let allowances_uref = get_allowances_uref();
    //     // let spender_allowance: U256 = read_allowance_from(allowances_uref, owner, spender);
    //     // let new_spender_allowance = spender_allowance
    //     //     .checked_sub(amount)
    //     //     .ok_or(Cep18Error::InsufficientAllowance)
    //     //     .unwrap_or_revert();

    //     // transfer_balance(owner, recipient, amount).unwrap_or_revert();
    //     // write_allowance_to(allowances_uref, owner, spender, new_spender_allowance);
    //     // events::record_event_dictionary(Event::TransferFrom(TransferFrom {
    //     //     spender,
    //     //     owner,
    //     //     recipient,
    //     //     amount,
    //     // }))
    // }

    // fn mint(&self, owner: Address, amount: u64) -> Result<(), Cep18Error> {
    //     todo!()
    //     // if 0 == read_from::<u8>(ENABLE_MINT_BURN) {
    //     //     revert(Cep18Error::MintBurnDisabled);
    //     // }

    //     // sec_check(vec![SecurityBadge::Admin, SecurityBadge::Minter]);

    //     // let owner: Key = runtime::get_named_arg(OWNER);
    //     // let amount: U256 = runtime::get_named_arg(AMOUNT);

    //     // let balances_uref = get_balances_uref();
    //     // let total_supply_uref = get_total_supply_uref();
    //     // let new_balance = {
    //     //     let balance = read_balance_from(balances_uref, owner);
    //     //     balance
    //     //         .checked_add(amount)
    //     //         .ok_or(Cep18Error::Overflow)
    //     //         .unwrap_or_revert()
    //     // };
    //     // let new_total_supply = {
    //     //     let total_supply: U256 = read_total_supply_from(total_supply_uref);
    //     //     total_supply
    //     //         .checked_add(amount)
    //     //         .ok_or(Cep18Error::Overflow)
    //     //         .unwrap_or_revert()
    //     // };
    //     // write_balance_to(balances_uref, owner, new_balance);
    //     // write_total_supply_to(total_supply_uref, new_total_supply);
    //     // events::record_event_dictionary(Event::Mint(Mint {
    //     //     recipient: owner,
    //     //     amount,
    //     // }))
    // }

    // fn burn(&self, owner: Address, amount: u64) -> Result<(), Cep18Error> {
    //     // if 0 == read_from::<u8>(ENABLE_MINT_BURN) {
    //     //     revert(Cep18Error::MintBurnDisabled);
    //     // }

    //     // let owner: Key = runtime::get_named_arg(OWNER);

    //     // if owner != get_immediate_caller_address().unwrap_or_revert() {
    //     //     revert(Cep18Error::InvalidBurnTarget);
    //     // }

    //     // let amount: U256 = runtime::get_named_arg(AMOUNT);
    //     // let balances_uref = get_balances_uref();
    //     // let total_supply_uref = get_total_supply_uref();
    //     // let new_balance = {
    //     //     let balance = read_balance_from(balances_uref, owner);
    //     //     balance
    //     //         .checked_sub(amount)
    //     //         .ok_or(Cep18Error::InsufficientBalance)
    //     //         .unwrap_or_revert()
    //     // };
    //     // let new_total_supply = {
    //     //     let total_supply = read_total_supply_from(total_supply_uref);
    //     //     total_supply
    //     //         .checked_sub(amount)
    //     //         .ok_or(Cep18Error::Overflow)
    //     //         .unwrap_or_revert()
    //     // };
    //     // write_balance_to(balances_uref, owner, new_balance);
    //     // write_total_supply_to(total_supply_uref, new_total_supply);
    //     // events::record_event_dictionary(Event::Burn(Burn { owner, amount }))
    //     todo!()
    // }
}

// /// Initiates the contracts states. Only used by the installer call,
// /// later calls will cause it to revert.
// fn init(&self) {
//     if get_key(ALLOWANCES).is_some() {
//         revert(Cep18Error::AlreadyInitialized);
//     }
//     let package_hash = get_named_arg::<Key>(PACKAGE_HASH);
//     put_key(PACKAGE_HASH, package_hash);
//     storage::new_dictionary(ALLOWANCES).unwrap_or_revert();
//     let balances_uref = storage::new_dictionary(BALANCES).unwrap_or_revert();
//     let initial_supply = runtime::get_named_arg(TOTAL_SUPPLY);
//     let caller = get_caller();
//     write_balance_to(balances_uref, caller.into(), initial_supply);

//     let security_badges_dict = storage::new_dictionary(SECURITY_BADGES).unwrap_or_revert();
//     dictionary_put(
//         security_badges_dict,
//         &base64::encode(Key::from(get_caller()).to_bytes().unwrap_or_revert()),
//         SecurityBadge::Admin,
//     );

//     let admin_list: Option<Vec<Key>> =
//         utils::get_optional_named_arg_with_user_errors(ADMIN_LIST, Cep18Error::InvalidAdminList);
//     let minter_list: Option<Vec<Key>> =
//         utils::get_optional_named_arg_with_user_errors(MINTER_LIST,
// Cep18Error::InvalidMinterList);

//     init_events();

//     if let Some(minter_list) = minter_list {
//         for minter in minter_list {
//             dictionary_put(
//                 security_badges_dict,
//                 &base64::encode(minter.to_bytes().unwrap_or_revert()),
//                 SecurityBadge::Minter,
//             );
//         }
//     }
//     if let Some(admin_list) = admin_list {
//         for admin in admin_list {
//             dictionary_put(
//                 security_badges_dict,
//                 &base64::encode(admin.to_bytes().unwrap_or_revert()),
//                 SecurityBadge::Admin,
//             );
//         }
//     }
// }

#[casper(export)]
pub fn call() {
    // let result = CEP18::create("new", None).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    use casper_sdk::{
        abi::CasperABI,
        host::native::{with_mock, Stub},
    };

    const DEFAULT_ACCOUNT: Address = [42; 32];
    const ALICE: Address = [1; 32];
    const BOB: Address = [2; 32];

    #[test]
    fn abi() {
        dbg!(CEP18::definition());
    }

    #[test]
    fn schema() {
        dbg!(CEP18::schema());
    }

    // #[test]
    // fn it_works() {
    //     let stub = Stub::new(Default::default(), [42; 32]);

    //     host::native::with_mock(new_stub, || {
    //         let mut contract = CEP18::new();
    //         assert_eq!(contract.name(), "Default name");
    //         assert_eq!(contract.balance_of(ALICE), 0);
    //         assert_eq!(contract.balance_of(BOB), 1);
    //         contract.approve(BOB, 111).unwrap();
    //     });
    // }
}
