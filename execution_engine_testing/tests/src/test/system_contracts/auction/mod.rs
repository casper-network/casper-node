use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{
    system::auction::{BidsExt, EraInfo, ValidatorBid},
    Key, PublicKey, U512,
};

mod bids;
mod distribute;
mod reservations;

fn get_validator_bid(
    builder: &mut LmdbWasmTestBuilder,
    validator_public_key: PublicKey,
) -> Option<ValidatorBid> {
    let bids = builder.get_bids();
    bids.validator_bid(&validator_public_key)
}

pub fn get_delegator_staked_amount(
    builder: &mut LmdbWasmTestBuilder,
    validator_public_key: PublicKey,
    delegator_public_key: PublicKey,
) -> U512 {
    let bids = builder.get_bids();
    let delegator = bids
        .delegator_by_public_keys(&validator_public_key, &delegator_public_key)
        .expect("bid should exist for validator-{validator_public_key}, delegator-{delegator_public_key}");

    delegator.staked_amount()
}

pub fn get_era_info(builder: &mut LmdbWasmTestBuilder) -> EraInfo {
    let era_info_value = builder
        .query(None, Key::EraSummary, &[])
        .expect("should have value");

    era_info_value
        .as_era_info()
        .cloned()
        .expect("should be era info")
}
