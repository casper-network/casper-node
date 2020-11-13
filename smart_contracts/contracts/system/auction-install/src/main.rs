#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    auction::{
        Bid, Bids, DelegatorRewardMap, EraId, SeigniorageRecipient, SeigniorageRecipients,
        SeigniorageRecipientsSnapshot, UnbondingPurses, ValidatorRewardMap, ValidatorWeights,
        ARG_AUCTION_DELAY, ARG_GENESIS_VALIDATORS, ARG_LOCKED_FUNDS_PERIOD,
        ARG_MINT_CONTRACT_PACKAGE_HASH, ARG_VALIDATOR_SLOTS, AUCTION_DELAY_KEY, BIDS_KEY,
        DELEGATOR_REWARD_MAP_KEY, DELEGATOR_REWARD_PURSE_KEY, ERA_ID_KEY, INITIAL_ERA_ID,
        LOCKED_FUNDS_PERIOD_KEY, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY, UNBONDING_PURSES_KEY,
        VALIDATOR_REWARD_MAP_KEY, VALIDATOR_REWARD_PURSE_KEY, VALIDATOR_SLOTS_KEY,
    },
    contracts::{NamedKeys, CONTRACT_INITIAL_VERSION},
    runtime_args,
    system_contract_errors::mint,
    CLValue, ContractPackageHash, PublicKey, RuntimeArgs, URef, U512,
};

const HASH_KEY_NAME: &str = "auction_hash";
const ACCESS_KEY_NAME: &str = "auction_access";
const ENTRY_POINT_MINT: &str = "mint";
const ARG_AMOUNT: &str = "amount";

#[no_mangle]
pub extern "C" fn install() {
    let mint_package_hash: ContractPackageHash =
        runtime::get_named_arg(ARG_MINT_CONTRACT_PACKAGE_HASH);

    let validator_slots: u32 = runtime::get_named_arg(ARG_VALIDATOR_SLOTS);
    let locked_funds_period: EraId = runtime::get_named_arg(ARG_LOCKED_FUNDS_PERIOD);

    let entry_points = auction::get_entry_points();
    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY_NAME, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY_NAME, access_uref.into());

    let named_keys = {
        let mut named_keys = NamedKeys::new();

        let mut validators = Bids::new();

        let genesis_validators: BTreeMap<PublicKey, U512> =
            runtime::get_named_arg(ARG_GENESIS_VALIDATORS);

        // List of validators for initial era.
        let mut initial_validator_weights = ValidatorWeights::new();

        for (validator_public_key, amount) in genesis_validators {
            let bonding_purse = create_purse(mint_package_hash, amount);
            let founding_validator = Bid::locked(bonding_purse, amount, locked_funds_period);
            validators.insert(validator_public_key, founding_validator);
            initial_validator_weights.insert(validator_public_key, amount);
        }

        let auction_delay: u64 = runtime::get_named_arg(ARG_AUCTION_DELAY);
        let initial_snapshot_range = INITIAL_ERA_ID..=INITIAL_ERA_ID + auction_delay;

        // Starting era validators
        named_keys.insert(ERA_ID_KEY.into(), storage::new_uref(INITIAL_ERA_ID).into());

        let seigniorage_recipients = compute_seigniorage_recipients(&validators);

        let mut initial_seigniorage_recipients = SeigniorageRecipientsSnapshot::new();
        for era_id in initial_snapshot_range {
            initial_seigniorage_recipients.insert(era_id, seigniorage_recipients.clone());
        }
        named_keys.insert(
            SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY.into(),
            storage::new_uref(initial_seigniorage_recipients).into(),
        );
        named_keys.insert(BIDS_KEY.into(), storage::new_uref(validators).into());
        named_keys.insert(
            UNBONDING_PURSES_KEY.into(),
            storage::new_uref(UnbondingPurses::new()).into(),
        );
        named_keys.insert(
            DELEGATOR_REWARD_PURSE_KEY.into(),
            create_purse(mint_package_hash, U512::zero()).into(),
        );
        named_keys.insert(
            VALIDATOR_REWARD_PURSE_KEY.into(),
            create_purse(mint_package_hash, U512::zero()).into(),
        );
        named_keys.insert(
            DELEGATOR_REWARD_MAP_KEY.into(),
            storage::new_uref(DelegatorRewardMap::new()).into(),
        );
        named_keys.insert(
            VALIDATOR_REWARD_MAP_KEY.into(),
            storage::new_uref(ValidatorRewardMap::new()).into(),
        );
        named_keys.insert(
            VALIDATOR_SLOTS_KEY.into(),
            storage::new_uref(validator_slots).into(),
        );
        named_keys.insert(
            AUCTION_DELAY_KEY.into(),
            storage::new_uref(auction_delay).into(),
        );

        named_keys.insert(
            LOCKED_FUNDS_PERIOD_KEY.into(),
            storage::new_uref(locked_funds_period).into(),
        );

        named_keys
    };

    let (contract_key, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    let return_value = CLValue::from_t(contract_key).unwrap_or_revert();
    runtime::ret(return_value);
}

fn compute_seigniorage_recipients(founding_validators: &Bids) -> SeigniorageRecipients {
    let mut seigniorage_recipients = SeigniorageRecipients::new();
    for (era_validator, founding_validator) in founding_validators {
        let seigniorage_recipient = SeigniorageRecipient::from(founding_validator);
        seigniorage_recipients.insert(*era_validator, seigniorage_recipient);
    }
    seigniorage_recipients
}

fn create_purse(contract_package_hash: ContractPackageHash, amount: U512) -> URef {
    let args = runtime_args! {
        ARG_AMOUNT => amount,
    };

    let result: Result<URef, mint::Error> = runtime::call_versioned_contract(
        contract_package_hash,
        Some(CONTRACT_INITIAL_VERSION),
        ENTRY_POINT_MINT,
        args,
    );

    result.unwrap_or_revert()
}
