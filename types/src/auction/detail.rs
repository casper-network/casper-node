use alloc::vec::Vec;

use super::{
    AuctionProvider, BidPurses, UnbondingPurses, BID_PURSES_KEY, SYSTEM_ACCOUNT,
    UNBONDING_PURSES_KEY,
};
use crate::{
    system_contract_errors::auction::{Error, Result},
    Key,
};

/// Iterates over unbonding entries and checks if a locked amount can be paid already if
/// a specific era is reached.
///
/// This function can be called by the system only.
pub(crate) fn process_unbond_requests<P: AuctionProvider + ?Sized>(provider: &mut P) -> Result<()> {
    if provider.get_caller() != SYSTEM_ACCOUNT {
        return Err(Error::InvalidCaller);
    }
    let bid_purses_uref = provider
        .get_key(BID_PURSES_KEY)
        .and_then(Key::into_uref)
        .ok_or(Error::MissingKey)?;

    let bid_purses: BidPurses = provider.read(bid_purses_uref)?.ok_or(Error::Storage)?;

    // Update `unbonding_purses` data
    let unbonding_purses_uref = provider
        .get_key(UNBONDING_PURSES_KEY)
        .and_then(Key::into_uref)
        .ok_or(Error::MissingKey)?;
    let mut unbonding_purses: UnbondingPurses = provider
        .read(unbonding_purses_uref)?
        .ok_or(Error::Storage)?;

    let current_era_id = provider.read_era_id()?;

    for unbonding_list in unbonding_purses.values_mut() {
        let mut new_unbonding_list = Vec::new();
        for unbonding_purse in unbonding_list.iter() {
            let source = bid_purses
                .get(&unbonding_purse.origin)
                .ok_or(Error::BondNotFound)?;
            // Since `process_unbond_requests` is run before `run_auction`, we should check
            // if current era id is equal or greater than the `era_of_withdrawal` that was
            // calculated on `unbond` attempt.
            if current_era_id >= unbonding_purse.era_of_withdrawal as u64 {
                // Move funds from bid purse to unbonding purse
                provider.transfer_from_purse_to_purse(
                    *source,
                    unbonding_purse.purse,
                    unbonding_purse.amount,
                )?;
            } else {
                new_unbonding_list.push(*unbonding_purse);
            }
        }
        *unbonding_list = new_unbonding_list;
    }

    // Prune empty entries
    let new_unbonding_purses: UnbondingPurses = unbonding_purses
        .into_iter()
        .filter(|(_k, unbonding_purses)| !unbonding_purses.is_empty())
        .collect();

    provider.write(unbonding_purses_uref, new_unbonding_purses)?;
    Ok(())
}
