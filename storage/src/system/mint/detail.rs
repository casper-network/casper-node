use casper_types::{
    system::{
        mint,
        mint::{Error, TOTAL_SUPPLY_KEY},
    },
    Key, U512,
};

use crate::system::mint::Mint;

// Please do not expose this to the user!
pub(crate) fn reduce_total_supply_unsafe<P>(mint: &mut P, amount: U512) -> Result<(), mint::Error>
where
    P: Mint + ?Sized,
{
    if amount.is_zero() {
        return Ok(()); // no change to supply
    }

    // get total supply or error
    let total_supply_uref = match mint.get_key(TOTAL_SUPPLY_KEY) {
        Some(Key::URef(uref)) => uref,
        Some(_) => return Err(Error::MissingKey), // TODO
        None => return Err(Error::MissingKey),
    };
    let total_supply: U512 = mint
        .read(total_supply_uref)?
        .ok_or(Error::TotalSupplyNotFound)?;

    // decrease total supply
    let reduced_total_supply = total_supply
        .checked_sub(amount)
        .ok_or(Error::ArithmeticOverflow)?;

    // update total supply
    mint.write_amount(total_supply_uref, reduced_total_supply)?;

    Ok(())
}
