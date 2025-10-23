use core::marker::PhantomData;

use crate::TransferTarget;

#[cfg(any(all(feature = "std", feature = "testing"), feature = "testing", test))]
use crate::PublicKey;
use crate::{bytesrepr::ToBytes, CLTyped, CLValueError, RuntimeArgs, URef, U512};
#[cfg(any(feature = "json-schema", feature = "testing", test))]
const TRANSFER_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");
#[cfg(any(feature = "json-schema", feature = "testing", test))]
const TRANSFER_ARG_SOURCE: OptionalArg<URef> = OptionalArg::new("source");
#[cfg(any(feature = "json-schema", feature = "testing", test))]
const TRANSFER_ARG_TARGET: &str = "target";
#[cfg(any(feature = "json-schema", feature = "testing", test))]
// "id" for legacy reasons, if the argument is passed it is [Option]
const TRANSFER_ARG_ID: OptionalArg<Option<u64>> = OptionalArg::new("id");

#[cfg(any(feature = "testing", test))]
const ADD_BID_ARG_PUBLIC_KEY: RequiredArg<PublicKey> = RequiredArg::new("public_key");
#[cfg(any(feature = "testing", test))]
const ADD_BID_ARG_DELEGATION_RATE: RequiredArg<u8> = RequiredArg::new("delegation_rate");
#[cfg(any(feature = "testing", test))]
const ADD_BID_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");
#[cfg(any(feature = "testing", test))]
const ADD_BID_ARG_MINIMUM_DELEGATION_AMOUNT: OptionalArg<u64> =
    OptionalArg::new("minimum_delegation_amount");
#[cfg(any(feature = "testing", test))]
const ADD_BID_ARG_MAXIMUM_DELEGATION_AMOUNT: OptionalArg<u64> =
    OptionalArg::new("maximum_delegation_amount");
#[cfg(any(feature = "testing", test))]
const ADD_BID_ARG_RESERVED_SLOTS: OptionalArg<u32> = OptionalArg::new("reserved_slots");
#[cfg(any(feature = "testing", test))]
const WITHDRAW_BID_ARG_PUBLIC_KEY: RequiredArg<PublicKey> = RequiredArg::new("public_key");
#[cfg(any(feature = "testing", test))]
const WITHDRAW_BID_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");
#[cfg(any(feature = "testing", test))]
const DELEGATE_ARG_DELEGATOR: RequiredArg<PublicKey> = RequiredArg::new("delegator");
#[cfg(any(feature = "testing", test))]
const DELEGATE_ARG_VALIDATOR: RequiredArg<PublicKey> = RequiredArg::new("validator");
#[cfg(any(feature = "testing", test))]
const DELEGATE_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");

#[cfg(any(feature = "testing", test))]
const UNDELEGATE_ARG_DELEGATOR: RequiredArg<PublicKey> = RequiredArg::new("delegator");
#[cfg(any(feature = "testing", test))]
const UNDELEGATE_ARG_VALIDATOR: RequiredArg<PublicKey> = RequiredArg::new("validator");
#[cfg(any(feature = "testing", test))]
const UNDELEGATE_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");

#[cfg(any(feature = "testing", test))]
const REDELEGATE_ARG_DELEGATOR: RequiredArg<PublicKey> = RequiredArg::new("delegator");
#[cfg(any(feature = "testing", test))]
const REDELEGATE_ARG_VALIDATOR: RequiredArg<PublicKey> = RequiredArg::new("validator");
#[cfg(any(feature = "testing", test))]
const REDELEGATE_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");
#[cfg(any(feature = "testing", test))]
const REDELEGATE_ARG_NEW_VALIDATOR: RequiredArg<PublicKey> = RequiredArg::new("new_validator");

struct RequiredArg<T> {
    name: &'static str,
    _phantom: PhantomData<T>,
}

impl<T> RequiredArg<T> {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            _phantom: PhantomData,
        }
    }

    fn insert(&self, args: &mut RuntimeArgs, value: T) -> Result<(), CLValueError>
    where
        T: CLTyped + ToBytes,
    {
        args.insert(self.name, value)
    }
}

struct OptionalArg<T> {
    name: &'static str,
    _phantom: PhantomData<T>,
}

impl<T> OptionalArg<T> {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            _phantom: PhantomData,
        }
    }

    fn insert(&self, args: &mut RuntimeArgs, value: T) -> Result<(), CLValueError>
    where
        T: CLTyped + ToBytes,
    {
        args.insert(self.name, value)
    }
}

#[cfg(any(feature = "json-schema", feature = "testing", test))]
/// Creates a `RuntimeArgs` suitable for use in a transfer transaction.
pub(crate) fn new_transfer_args<A: Into<U512>, T: Into<TransferTarget>>(
    amount: A,
    maybe_source: Option<URef>,
    target: T,
    maybe_id: Option<u64>,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    if let Some(source) = maybe_source {
        TRANSFER_ARG_SOURCE.insert(&mut args, source)?;
    }
    match target.into() {
        TransferTarget::PublicKey(public_key) => args.insert(TRANSFER_ARG_TARGET, public_key)?,
        TransferTarget::AccountHash(account_hash) => {
            args.insert(TRANSFER_ARG_TARGET, account_hash)?
        }
        TransferTarget::URef(uref) => args.insert(TRANSFER_ARG_TARGET, uref)?,
    }
    TRANSFER_ARG_AMOUNT.insert(&mut args, amount.into())?;
    if maybe_id.is_some() {
        TRANSFER_ARG_ID.insert(&mut args, maybe_id)?;
    }
    Ok(args)
}

/// Creates a `RuntimeArgs` suitable for use in an add_bid transaction.
#[cfg(any(all(feature = "std", feature = "testing"), feature = "testing", test))]
pub(crate) fn new_add_bid_args<A: Into<U512>>(
    public_key: PublicKey,
    delegation_rate: u8,
    amount: A,
    maybe_minimum_delegation_amount: Option<u64>,
    maybe_maximum_delegation_amount: Option<u64>,
    maybe_reserved_slots: Option<u32>,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    ADD_BID_ARG_PUBLIC_KEY.insert(&mut args, public_key)?;
    ADD_BID_ARG_DELEGATION_RATE.insert(&mut args, delegation_rate)?;
    ADD_BID_ARG_AMOUNT.insert(&mut args, amount.into())?;
    if let Some(minimum_delegation_amount) = maybe_minimum_delegation_amount {
        ADD_BID_ARG_MINIMUM_DELEGATION_AMOUNT.insert(&mut args, minimum_delegation_amount)?;
    };
    if let Some(maximum_delegation_amount) = maybe_maximum_delegation_amount {
        ADD_BID_ARG_MAXIMUM_DELEGATION_AMOUNT.insert(&mut args, maximum_delegation_amount)?;
    };
    if let Some(reserved_slots) = maybe_reserved_slots {
        ADD_BID_ARG_RESERVED_SLOTS.insert(&mut args, reserved_slots)?;
    };
    Ok(args)
}

/// Creates a `RuntimeArgs` suitable for use in a withdraw_bid transaction.
#[cfg(any(all(feature = "std", feature = "testing"), feature = "testing", test))]
pub fn new_withdraw_bid_args<A: Into<U512>>(
    public_key: PublicKey,
    amount: A,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    WITHDRAW_BID_ARG_PUBLIC_KEY.insert(&mut args, public_key)?;
    WITHDRAW_BID_ARG_AMOUNT.insert(&mut args, amount.into())?;
    Ok(args)
}

/// Creates a `RuntimeArgs` suitable for use in a delegate transaction.
#[cfg(any(all(feature = "std", feature = "testing"), feature = "testing", test))]
pub(crate) fn new_delegate_args<A: Into<U512>>(
    delegator: PublicKey,
    validator: PublicKey,
    amount: A,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    DELEGATE_ARG_DELEGATOR.insert(&mut args, delegator)?;
    DELEGATE_ARG_VALIDATOR.insert(&mut args, validator)?;
    DELEGATE_ARG_AMOUNT.insert(&mut args, amount.into())?;
    Ok(args)
}

/// Creates a `RuntimeArgs` suitable for use in an undelegate transaction.
#[cfg(any(all(feature = "std", feature = "testing"), feature = "testing", test))]
pub(crate) fn new_undelegate_args<A: Into<U512>>(
    delegator: PublicKey,
    validator: PublicKey,
    amount: A,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    UNDELEGATE_ARG_DELEGATOR.insert(&mut args, delegator)?;
    UNDELEGATE_ARG_VALIDATOR.insert(&mut args, validator)?;
    UNDELEGATE_ARG_AMOUNT.insert(&mut args, amount.into())?;
    Ok(args)
}

/// Creates a `RuntimeArgs` suitable for use in a redelegate transaction.
#[cfg(any(all(feature = "std", feature = "testing"), feature = "testing", test))]
pub(crate) fn new_redelegate_args<A: Into<U512>>(
    delegator: PublicKey,
    validator: PublicKey,
    amount: A,
    new_validator: PublicKey,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    REDELEGATE_ARG_DELEGATOR.insert(&mut args, delegator)?;
    REDELEGATE_ARG_VALIDATOR.insert(&mut args, validator)?;
    REDELEGATE_ARG_AMOUNT.insert(&mut args, amount.into())?;
    REDELEGATE_ARG_NEW_VALIDATOR.insert(&mut args, new_validator)?;
    Ok(args)
}
