use core::marker::PhantomData;

use tracing::debug;

use super::super::TransactionV1ConfigFailure;
use crate::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    CLTyped, CLValue, CLValueError, PublicKey, RuntimeArgs, URef, U512,
};

const TRANSFER_ARG_SOURCE: RequiredArg<URef> = RequiredArg::new("source");
const TRANSFER_ARG_TARGET: RequiredArg<URef> = RequiredArg::new("target");
const TRANSFER_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");
const TRANSFER_ARG_TO: OptionalArg<AccountHash> = OptionalArg::new("to");
const TRANSFER_ARG_ID: OptionalArg<u64> = OptionalArg::new("id");

const ADD_BID_ARG_PUBLIC_KEY: RequiredArg<PublicKey> = RequiredArg::new("public_key");
const ADD_BID_ARG_DELEGATION_RATE: RequiredArg<u8> = RequiredArg::new("delegation_rate");
const ADD_BID_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");

const WITHDRAW_BID_ARG_PUBLIC_KEY: RequiredArg<PublicKey> = RequiredArg::new("public_key");
const WITHDRAW_BID_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");

const DELEGATE_ARG_DELEGATOR: RequiredArg<PublicKey> = RequiredArg::new("delegator");
const DELEGATE_ARG_VALIDATOR: RequiredArg<PublicKey> = RequiredArg::new("validator");
const DELEGATE_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");

const UNDELEGATE_ARG_DELEGATOR: RequiredArg<PublicKey> = RequiredArg::new("delegator");
const UNDELEGATE_ARG_VALIDATOR: RequiredArg<PublicKey> = RequiredArg::new("validator");
const UNDELEGATE_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");

const REDELEGATE_ARG_DELEGATOR: RequiredArg<PublicKey> = RequiredArg::new("delegator");
const REDELEGATE_ARG_VALIDATOR: RequiredArg<PublicKey> = RequiredArg::new("validator");
const REDELEGATE_ARG_AMOUNT: RequiredArg<U512> = RequiredArg::new("amount");
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

    fn get(&self, args: &RuntimeArgs) -> Result<T, TransactionV1ConfigFailure>
    where
        T: CLTyped + FromBytes,
    {
        let cl_value = args.get(self.name).ok_or_else(|| {
            debug!("missing required runtime argument '{}'", self.name);
            TransactionV1ConfigFailure::MissingArg {
                arg_name: self.name.to_string(),
            }
        })?;
        parse_cl_value(cl_value, self.name)
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

    fn get(&self, args: &RuntimeArgs) -> Result<Option<T>, TransactionV1ConfigFailure>
    where
        T: CLTyped + FromBytes,
    {
        let cl_value = match args.get(self.name) {
            Some(value) => value,
            None => return Ok(None),
        };
        let value = parse_cl_value(cl_value, self.name)?;
        Ok(value)
    }

    fn insert(&self, args: &mut RuntimeArgs, value: T) -> Result<(), CLValueError>
    where
        T: CLTyped + ToBytes,
    {
        args.insert(self.name, Some(value))
    }
}

fn parse_cl_value<T: CLTyped + FromBytes>(
    cl_value: &CLValue,
    arg_name: &str,
) -> Result<T, TransactionV1ConfigFailure> {
    cl_value.to_t::<T>().map_err(|_| {
        debug!(
            "expected runtime argument '{arg_name}' to be of type {}, but is {}",
            T::cl_type(),
            cl_value.cl_type()
        );
        TransactionV1ConfigFailure::UnexpectedArgType {
            arg_name: arg_name.to_string(),
            expected: T::cl_type(),
            got: cl_value.cl_type().clone(),
        }
    })
}

/// Creates a `RuntimeArgs` suitable for use in a transfer transaction.
pub(in crate::transaction::transaction_v1) fn new_transfer_args<A: Into<U512>>(
    source: URef,
    target: URef,
    amount: A,
    maybe_to: Option<AccountHash>,
    maybe_id: Option<u64>,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    TRANSFER_ARG_SOURCE.insert(&mut args, source)?;
    TRANSFER_ARG_TARGET.insert(&mut args, target)?;
    TRANSFER_ARG_AMOUNT.insert(&mut args, amount.into())?;
    if let Some(to) = maybe_to {
        TRANSFER_ARG_TO.insert(&mut args, to)?;
    }
    if let Some(id) = maybe_id {
        TRANSFER_ARG_ID.insert(&mut args, id)?;
    }
    Ok(args)
}

/// Checks the given `RuntimeArgs` are suitable for use in a transfer transaction.
pub(in crate::transaction::transaction_v1) fn has_valid_transfer_args(
    args: &RuntimeArgs,
    native_transfer_minimum_motes: u64,
) -> Result<(), TransactionV1ConfigFailure> {
    let _source = TRANSFER_ARG_SOURCE.get(args)?;
    let _target = TRANSFER_ARG_TARGET.get(args)?;
    let amount = TRANSFER_ARG_AMOUNT.get(args)?;
    if amount < U512::from(native_transfer_minimum_motes) {
        debug!(
            minimum = %native_transfer_minimum_motes,
            %amount,
            "insufficient transfer amount"
        );
        return Err(TransactionV1ConfigFailure::InsufficientTransferAmount {
            minimum: native_transfer_minimum_motes,
            attempted: amount,
        });
    }
    let _maybe_to = TRANSFER_ARG_TO.get(args)?;
    let _maybe_id = TRANSFER_ARG_ID.get(args)?;
    Ok(())
}

/// Creates a `RuntimeArgs` suitable for use in an add_bid transaction.
pub(in crate::transaction::transaction_v1) fn new_add_bid_args<A: Into<U512>>(
    public_key: PublicKey,
    delegation_rate: u8,
    amount: A,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    ADD_BID_ARG_PUBLIC_KEY.insert(&mut args, public_key)?;
    ADD_BID_ARG_DELEGATION_RATE.insert(&mut args, delegation_rate)?;
    ADD_BID_ARG_AMOUNT.insert(&mut args, amount.into())?;
    Ok(args)
}

/// Checks the given `RuntimeArgs` are suitable for use in an add_bid transaction.
pub(in crate::transaction::transaction_v1) fn has_valid_add_bid_args(
    args: &RuntimeArgs,
) -> Result<(), TransactionV1ConfigFailure> {
    let _public_key = ADD_BID_ARG_PUBLIC_KEY.get(args)?;
    let _delegation_rate = ADD_BID_ARG_DELEGATION_RATE.get(args)?;
    let _amount = ADD_BID_ARG_AMOUNT.get(args)?;
    Ok(())
}

/// Creates a `RuntimeArgs` suitable for use in a withdraw_bid transaction.
pub(in crate::transaction::transaction_v1) fn new_withdraw_bid_args<A: Into<U512>>(
    public_key: PublicKey,
    amount: A,
) -> Result<RuntimeArgs, CLValueError> {
    let mut args = RuntimeArgs::new();
    WITHDRAW_BID_ARG_PUBLIC_KEY.insert(&mut args, public_key)?;
    WITHDRAW_BID_ARG_AMOUNT.insert(&mut args, amount.into())?;
    Ok(args)
}

/// Checks the given `RuntimeArgs` are suitable for use in an withdraw_bid transaction.
pub(in crate::transaction::transaction_v1) fn has_valid_withdraw_bid_args(
    args: &RuntimeArgs,
) -> Result<(), TransactionV1ConfigFailure> {
    let _public_key = WITHDRAW_BID_ARG_PUBLIC_KEY.get(args)?;
    let _amount = WITHDRAW_BID_ARG_AMOUNT.get(args)?;
    Ok(())
}

/// Creates a `RuntimeArgs` suitable for use in a delegate transaction.
pub(in crate::transaction::transaction_v1) fn new_delegate_args<A: Into<U512>>(
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

/// Checks the given `RuntimeArgs` are suitable for use in a delegate transaction.
pub(in crate::transaction::transaction_v1) fn has_valid_delegate_args(
    args: &RuntimeArgs,
) -> Result<(), TransactionV1ConfigFailure> {
    let _delegator = DELEGATE_ARG_DELEGATOR.get(args)?;
    let _validator = DELEGATE_ARG_VALIDATOR.get(args)?;
    let _amount = DELEGATE_ARG_AMOUNT.get(args)?;
    Ok(())
}

/// Creates a `RuntimeArgs` suitable for use in an undelegate transaction.
pub(in crate::transaction::transaction_v1) fn new_undelegate_args<A: Into<U512>>(
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

/// Checks the given `RuntimeArgs` are suitable for use in an undelegate transaction.
pub(in crate::transaction::transaction_v1) fn has_valid_undelegate_args(
    args: &RuntimeArgs,
) -> Result<(), TransactionV1ConfigFailure> {
    let _delegator = UNDELEGATE_ARG_DELEGATOR.get(args)?;
    let _validator = UNDELEGATE_ARG_VALIDATOR.get(args)?;
    let _amount = UNDELEGATE_ARG_AMOUNT.get(args)?;
    Ok(())
}

/// Creates a `RuntimeArgs` suitable for use in a redelegate transaction.
pub(in crate::transaction::transaction_v1) fn new_redelegate_args<A: Into<U512>>(
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

/// Checks the given `RuntimeArgs` are suitable for use in a redelegate transaction.
pub(in crate::transaction::transaction_v1) fn has_valid_redelegate_args(
    args: &RuntimeArgs,
) -> Result<(), TransactionV1ConfigFailure> {
    let _delegator = REDELEGATE_ARG_DELEGATOR.get(args)?;
    let _validator = REDELEGATE_ARG_VALIDATOR.get(args)?;
    let _amount = REDELEGATE_ARG_AMOUNT.get(args)?;
    let _new_validator = REDELEGATE_ARG_NEW_VALIDATOR.get(args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;
    use crate::{runtime_args, testing::TestRng, CLType};

    #[test]
    fn should_validate_transfer_args() {
        let rng = &mut TestRng::new();
        let min_motes = 10_u64;
        // Check random args, within motes limit.
        let args = new_transfer_args(
            rng.gen(),
            rng.gen(),
            U512::from(rng.gen_range(min_motes..=u64::MAX)),
            rng.gen::<bool>().then(|| rng.gen()),
            rng.gen::<bool>().then(|| rng.gen()),
        )
        .unwrap();
        has_valid_transfer_args(&args, min_motes).unwrap();

        // Check at minimum motes limit.
        let args = new_transfer_args(
            rng.gen(),
            rng.gen(),
            U512::from(min_motes),
            rng.gen::<bool>().then(|| rng.gen()),
            rng.gen::<bool>().then(|| rng.gen()),
        )
        .unwrap();
        has_valid_transfer_args(&args, min_motes).unwrap();

        // Check with extra arg.
        let mut args = new_transfer_args(
            rng.gen(),
            rng.gen(),
            U512::from(min_motes),
            rng.gen::<bool>().then(|| rng.gen()),
            rng.gen::<bool>().then(|| rng.gen()),
        )
        .unwrap();
        args.insert("a", 1).unwrap();
        has_valid_transfer_args(&args, min_motes).unwrap();
    }

    #[test]
    fn transfer_args_with_low_amount_should_be_invalid() {
        let rng = &mut TestRng::new();
        let min_motes = 10_u64;

        let args = runtime_args! {
            TRANSFER_ARG_SOURCE.name => rng.gen::<URef>(),
            TRANSFER_ARG_TARGET.name => rng.gen::<URef>(),
            TRANSFER_ARG_AMOUNT.name => U512::from(min_motes - 1)
        };

        let expected_error = TransactionV1ConfigFailure::InsufficientTransferAmount {
            minimum: min_motes,
            attempted: U512::from(min_motes - 1),
        };

        assert_eq!(
            has_valid_transfer_args(&args, min_motes),
            Err(expected_error)
        );
    }

    #[test]
    fn transfer_args_with_missing_required_should_be_invalid() {
        let rng = &mut TestRng::new();
        let min_motes = 10_u64;

        // Missing "source".
        let args = runtime_args! {
            TRANSFER_ARG_TARGET.name => rng.gen::<URef>(),
            TRANSFER_ARG_AMOUNT.name => U512::from(min_motes)
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: TRANSFER_ARG_SOURCE.name.to_string(),
        };
        assert_eq!(
            has_valid_transfer_args(&args, min_motes),
            Err(expected_error)
        );

        // Missing "target".
        let args = runtime_args! {
            TRANSFER_ARG_SOURCE.name => rng.gen::<URef>(),
            TRANSFER_ARG_AMOUNT.name => U512::from(min_motes)
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: TRANSFER_ARG_TARGET.name.to_string(),
        };
        assert_eq!(
            has_valid_transfer_args(&args, min_motes),
            Err(expected_error)
        );

        // Missing "amount".
        let args = runtime_args! {
            TRANSFER_ARG_SOURCE.name => rng.gen::<URef>(),
            TRANSFER_ARG_TARGET.name => rng.gen::<URef>()
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: TRANSFER_ARG_AMOUNT.name.to_string(),
        };
        assert_eq!(
            has_valid_transfer_args(&args, min_motes),
            Err(expected_error)
        );
    }

    #[test]
    fn transfer_args_with_wrong_type_should_be_invalid() {
        let rng = &mut TestRng::new();
        let min_motes = 10_u64;

        // Wrong "source" type (a required arg).
        let args = runtime_args! {
            TRANSFER_ARG_SOURCE.name => 1_u8,
            TRANSFER_ARG_TARGET.name => rng.gen::<URef>(),
            TRANSFER_ARG_AMOUNT.name => U512::from(min_motes)
        };
        let expected_error = TransactionV1ConfigFailure::UnexpectedArgType {
            arg_name: TRANSFER_ARG_SOURCE.name.to_string(),
            expected: CLType::URef,
            got: CLType::U8,
        };
        assert_eq!(
            has_valid_transfer_args(&args, min_motes),
            Err(expected_error)
        );

        // Wrong "to" type (an optional arg).
        let args = runtime_args! {
            TRANSFER_ARG_SOURCE.name => rng.gen::<URef>(),
            TRANSFER_ARG_TARGET.name => rng.gen::<URef>(),
            TRANSFER_ARG_AMOUNT.name => U512::from(min_motes),
            TRANSFER_ARG_TO.name => 1_u8
        };
        let expected_error = TransactionV1ConfigFailure::UnexpectedArgType {
            arg_name: TRANSFER_ARG_TO.name.to_string(),
            expected: Option::<AccountHash>::cl_type(),
            got: CLType::U8,
        };
        assert_eq!(
            has_valid_transfer_args(&args, min_motes),
            Err(expected_error)
        );
    }

    #[test]
    fn should_validate_add_bid_args() {
        let rng = &mut TestRng::new();

        // Check random args.
        let mut args =
            new_add_bid_args(PublicKey::random(rng), rng.gen(), rng.gen::<u64>()).unwrap();
        has_valid_add_bid_args(&args).unwrap();

        // Check with extra arg.
        args.insert("a", 1).unwrap();
        has_valid_add_bid_args(&args).unwrap();
    }

    #[test]
    fn add_bid_args_with_missing_required_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Missing "public_key".
        let args = runtime_args! {
            ADD_BID_ARG_DELEGATION_RATE.name => rng.gen::<u8>(),
            ADD_BID_ARG_AMOUNT.name => U512::from(rng.gen::<u64>())
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: ADD_BID_ARG_PUBLIC_KEY.name.to_string(),
        };
        assert_eq!(has_valid_add_bid_args(&args), Err(expected_error));

        // Missing "delegation_rate".
        let args = runtime_args! {
            ADD_BID_ARG_PUBLIC_KEY.name => PublicKey::random(rng),
            ADD_BID_ARG_AMOUNT.name => U512::from(rng.gen::<u64>())
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: ADD_BID_ARG_DELEGATION_RATE.name.to_string(),
        };
        assert_eq!(has_valid_add_bid_args(&args), Err(expected_error));

        // Missing "amount".
        let args = runtime_args! {
            ADD_BID_ARG_PUBLIC_KEY.name => PublicKey::random(rng),
            ADD_BID_ARG_DELEGATION_RATE.name => rng.gen::<u8>()
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: ADD_BID_ARG_AMOUNT.name.to_string(),
        };
        assert_eq!(has_valid_add_bid_args(&args), Err(expected_error));
    }

    #[test]
    fn add_bid_args_with_wrong_type_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Wrong "amount" type.
        let args = runtime_args! {
            ADD_BID_ARG_PUBLIC_KEY.name => PublicKey::random(rng),
            ADD_BID_ARG_DELEGATION_RATE.name => rng.gen::<u8>(),
            ADD_BID_ARG_AMOUNT.name => rng.gen::<u64>()
        };
        let expected_error = TransactionV1ConfigFailure::UnexpectedArgType {
            arg_name: ADD_BID_ARG_AMOUNT.name.to_string(),
            expected: CLType::U512,
            got: CLType::U64,
        };
        assert_eq!(has_valid_add_bid_args(&args), Err(expected_error));
    }

    #[test]
    fn should_validate_withdraw_bid_args() {
        let rng = &mut TestRng::new();

        // Check random args.
        let mut args = new_withdraw_bid_args(PublicKey::random(rng), rng.gen::<u64>()).unwrap();
        has_valid_withdraw_bid_args(&args).unwrap();

        // Check with extra arg.
        args.insert("a", 1).unwrap();
        has_valid_withdraw_bid_args(&args).unwrap();
    }

    #[test]
    fn withdraw_bid_args_with_missing_required_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Missing "public_key".
        let args = runtime_args! {
            WITHDRAW_BID_ARG_AMOUNT.name => U512::from(rng.gen::<u64>())
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: WITHDRAW_BID_ARG_PUBLIC_KEY.name.to_string(),
        };
        assert_eq!(has_valid_withdraw_bid_args(&args), Err(expected_error));

        // Missing "amount".
        let args = runtime_args! {
            WITHDRAW_BID_ARG_PUBLIC_KEY.name => PublicKey::random(rng),
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: WITHDRAW_BID_ARG_AMOUNT.name.to_string(),
        };
        assert_eq!(has_valid_withdraw_bid_args(&args), Err(expected_error));
    }

    #[test]
    fn withdraw_bid_args_with_wrong_type_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Wrong "amount" type.
        let args = runtime_args! {
            WITHDRAW_BID_ARG_PUBLIC_KEY.name => PublicKey::random(rng),
            WITHDRAW_BID_ARG_AMOUNT.name => rng.gen::<u64>()
        };
        let expected_error = TransactionV1ConfigFailure::UnexpectedArgType {
            arg_name: WITHDRAW_BID_ARG_AMOUNT.name.to_string(),
            expected: CLType::U512,
            got: CLType::U64,
        };
        assert_eq!(has_valid_withdraw_bid_args(&args), Err(expected_error));
    }

    #[test]
    fn should_validate_delegate_args() {
        let rng = &mut TestRng::new();

        // Check random args.
        let mut args = new_delegate_args(
            PublicKey::random(rng),
            PublicKey::random(rng),
            rng.gen::<u64>(),
        )
        .unwrap();
        has_valid_delegate_args(&args).unwrap();

        // Check with extra arg.
        args.insert("a", 1).unwrap();
        has_valid_delegate_args(&args).unwrap();
    }

    #[test]
    fn delegate_args_with_missing_required_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Missing "delegator".
        let args = runtime_args! {
            DELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
            DELEGATE_ARG_AMOUNT.name => U512::from(rng.gen::<u64>())
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: DELEGATE_ARG_DELEGATOR.name.to_string(),
        };
        assert_eq!(has_valid_delegate_args(&args), Err(expected_error));

        // Missing "validator".
        let args = runtime_args! {
            DELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            DELEGATE_ARG_AMOUNT.name => U512::from(rng.gen::<u64>())
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: DELEGATE_ARG_VALIDATOR.name.to_string(),
        };
        assert_eq!(has_valid_delegate_args(&args), Err(expected_error));

        // Missing "amount".
        let args = runtime_args! {
            DELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            DELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: DELEGATE_ARG_AMOUNT.name.to_string(),
        };
        assert_eq!(has_valid_delegate_args(&args), Err(expected_error));
    }

    #[test]
    fn delegate_args_with_wrong_type_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Wrong "amount" type.
        let args = runtime_args! {
            DELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            DELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
            DELEGATE_ARG_AMOUNT.name => rng.gen::<u64>()
        };
        let expected_error = TransactionV1ConfigFailure::UnexpectedArgType {
            arg_name: DELEGATE_ARG_AMOUNT.name.to_string(),
            expected: CLType::U512,
            got: CLType::U64,
        };
        assert_eq!(has_valid_delegate_args(&args), Err(expected_error));
    }

    #[test]
    fn should_validate_undelegate_args() {
        let rng = &mut TestRng::new();

        // Check random args.
        let mut args = new_undelegate_args(
            PublicKey::random(rng),
            PublicKey::random(rng),
            rng.gen::<u64>(),
        )
        .unwrap();
        has_valid_undelegate_args(&args).unwrap();

        // Check with extra arg.
        args.insert("a", 1).unwrap();
        has_valid_undelegate_args(&args).unwrap();
    }

    #[test]
    fn undelegate_args_with_missing_required_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Missing "delegator".
        let args = runtime_args! {
            UNDELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
            UNDELEGATE_ARG_AMOUNT.name => U512::from(rng.gen::<u64>())
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: UNDELEGATE_ARG_DELEGATOR.name.to_string(),
        };
        assert_eq!(has_valid_undelegate_args(&args), Err(expected_error));

        // Missing "validator".
        let args = runtime_args! {
            UNDELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            UNDELEGATE_ARG_AMOUNT.name => U512::from(rng.gen::<u64>())
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: UNDELEGATE_ARG_VALIDATOR.name.to_string(),
        };
        assert_eq!(has_valid_undelegate_args(&args), Err(expected_error));

        // Missing "amount".
        let args = runtime_args! {
            UNDELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            UNDELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: UNDELEGATE_ARG_AMOUNT.name.to_string(),
        };
        assert_eq!(has_valid_undelegate_args(&args), Err(expected_error));
    }

    #[test]
    fn undelegate_args_with_wrong_type_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Wrong "amount" type.
        let args = runtime_args! {
            UNDELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            UNDELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
            UNDELEGATE_ARG_AMOUNT.name => rng.gen::<u64>()
        };
        let expected_error = TransactionV1ConfigFailure::UnexpectedArgType {
            arg_name: UNDELEGATE_ARG_AMOUNT.name.to_string(),
            expected: CLType::U512,
            got: CLType::U64,
        };
        assert_eq!(has_valid_undelegate_args(&args), Err(expected_error));
    }

    #[test]
    fn should_validate_redelegate_args() {
        let rng = &mut TestRng::new();

        // Check random args.
        let mut args = new_redelegate_args(
            PublicKey::random(rng),
            PublicKey::random(rng),
            rng.gen::<u64>(),
            PublicKey::random(rng),
        )
        .unwrap();
        has_valid_redelegate_args(&args).unwrap();

        // Check with extra arg.
        args.insert("a", 1).unwrap();
        has_valid_redelegate_args(&args).unwrap();
    }

    #[test]
    fn redelegate_args_with_missing_required_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Missing "delegator".
        let args = runtime_args! {
            REDELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
            REDELEGATE_ARG_AMOUNT.name => U512::from(rng.gen::<u64>()),
            REDELEGATE_ARG_NEW_VALIDATOR.name => PublicKey::random(rng),
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: REDELEGATE_ARG_DELEGATOR.name.to_string(),
        };
        assert_eq!(has_valid_redelegate_args(&args), Err(expected_error));

        // Missing "validator".
        let args = runtime_args! {
            REDELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            REDELEGATE_ARG_AMOUNT.name => U512::from(rng.gen::<u64>()),
            REDELEGATE_ARG_NEW_VALIDATOR.name => PublicKey::random(rng),
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: REDELEGATE_ARG_VALIDATOR.name.to_string(),
        };
        assert_eq!(has_valid_redelegate_args(&args), Err(expected_error));

        // Missing "amount".
        let args = runtime_args! {
            REDELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            REDELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
            REDELEGATE_ARG_NEW_VALIDATOR.name => PublicKey::random(rng),
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: REDELEGATE_ARG_AMOUNT.name.to_string(),
        };
        assert_eq!(has_valid_redelegate_args(&args), Err(expected_error));

        // Missing "new_validator".
        let args = runtime_args! {
            REDELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            REDELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
            REDELEGATE_ARG_AMOUNT.name => U512::from(rng.gen::<u64>()),
        };
        let expected_error = TransactionV1ConfigFailure::MissingArg {
            arg_name: REDELEGATE_ARG_NEW_VALIDATOR.name.to_string(),
        };
        assert_eq!(has_valid_redelegate_args(&args), Err(expected_error));
    }

    #[test]
    fn redelegate_args_with_wrong_type_should_be_invalid() {
        let rng = &mut TestRng::new();

        // Wrong "amount" type.
        let args = runtime_args! {
            REDELEGATE_ARG_DELEGATOR.name => PublicKey::random(rng),
            REDELEGATE_ARG_VALIDATOR.name => PublicKey::random(rng),
            REDELEGATE_ARG_AMOUNT.name => rng.gen::<u64>(),
            REDELEGATE_ARG_NEW_VALIDATOR.name => PublicKey::random(rng),
        };
        let expected_error = TransactionV1ConfigFailure::UnexpectedArgType {
            arg_name: REDELEGATE_ARG_AMOUNT.name.to_string(),
            expected: CLType::U512,
            got: CLType::U64,
        };
        assert_eq!(has_valid_redelegate_args(&args), Err(expected_error));
    }
}
