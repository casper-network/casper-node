use casper_types::{
    system::{mint, mint::TOTAL_SUPPLY_KEY},
    Key, U512,
};

use crate::system::mint::{runtime_provider::RuntimeProvider, storage_provider::StorageProvider};

// Please do not expose this to the user!
pub(crate) fn reduce_total_supply_unchecked<P>(
    auction: &mut P,
    amount: U512,
) -> Result<(), mint::Error>
where
    P: StorageProvider + RuntimeProvider + ?Sized,
{
    if amount.is_zero() {
        return Ok(()); // no change to supply
    }

    // get total supply or error
    let total_supply_uref = match auction.get_key(TOTAL_SUPPLY_KEY) {
        Some(Key::URef(uref)) => uref,
        Some(_) => return Err(mint::Error::MissingKey), // TODO
        None => return Err(mint::Error::MissingKey),
    };
    let total_supply: U512 = auction
        .read(total_supply_uref)?
        .ok_or(mint::Error::TotalSupplyNotFound)?;

    // decrease total supply
    let reduced_total_supply = total_supply
        .checked_sub(amount)
        .ok_or(mint::Error::ArithmeticOverflow)?;

    // update total supply
    auction.write(total_supply_uref, reduced_total_supply)?;

    Ok(())
}
