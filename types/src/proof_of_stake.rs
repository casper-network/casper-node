//! Contains implementation of a Proof Of Stake contract functionality.
mod auction_provider;
mod mint_provider;
mod queue;
mod queue_provider;
mod runtime_provider;
mod stakes;
mod stakes_provider;

use alloc::collections::BTreeMap;
use core::marker::Sized;

use crate::{
    account::AccountHash,
    system_contract_errors::pos::{Error, Result},
    AccessRights, TransferredTo, URef, U512,
};

pub use crate::proof_of_stake::{
    auction_provider::AuctionProvider, mint_provider::MintProvider, queue::Queue,
    queue_provider::QueueProvider, runtime_provider::RuntimeProvider, stakes::Stakes,
    stakes_provider::StakesProvider,
};

use crate::{PublicKey, auction::{COMMISSION_RATE_DENOMINATOR, DelegationProvider}};

/// Proof of stake functionality implementation.
pub trait ProofOfStake:
    MintProvider + QueueProvider + RuntimeProvider + StakesProvider + AuctionProvider + Sized
{
    /// Bonds a `validator` with `amount` of tokens from a `source` purse.
    fn bond_old(&mut self, validator: AccountHash, amount: U512, source: URef) -> Result<()> {
        if amount.is_zero() {
            return Err(Error::BondTooSmall);
        }
        let target = internal::get_bonding_purse(self)?;
        let timestamp = self.get_block_time();
        // Transfer `amount` from the `source` purse to PoS internal purse. POS_PURSE is a constant,
        // it is the URef of the proof-of-stake contract's own purse.

        self.transfer_purse_to_purse(source, target, amount)
            .map_err(|_| Error::BondTransferFailed)?;
        internal::bond(self, amount, validator, timestamp)?;

        // TODO: Remove this and set nonzero delays once the system calls `step` in each block.
        let unbonds = internal::step(self, timestamp)?;
        for entry in unbonds {
            let _: TransferredTo = self
                .transfer_purse_to_account(source, entry.validator, entry.amount)
                .map_err(|_| Error::BondTransferFailed)?;
        }
        Ok(())
    }

    /// Unbonds a validator with provided `maybe_amount`. If `maybe_amount` is `None` then given
    /// validator will withdraw all funds.
    fn unbond_old(&mut self, validator: AccountHash, maybe_amount: Option<U512>) -> Result<()> {
        let pos_purse = internal::get_bonding_purse(self)?;
        let timestamp = self.get_block_time();
        internal::unbond(self, maybe_amount, validator, timestamp)?;

        // TODO: Remove this and set nonzero delays once the system calls `step` in each block.
        let unbonds = internal::step(self, timestamp)?;
        for entry in unbonds {
            self.transfer_purse_to_account(pos_purse, entry.validator, entry.amount)
                .map_err(|_| Error::UnbondTransferFailed)?;
        }
        Ok(())
    }

    /// Get payment purse.
    fn get_payment_purse(&self) -> Result<URef> {
        let purse = internal::get_payment_purse(self)?;
        // Limit the access rights so only balance query and deposit are allowed.
        Ok(URef::new(purse.addr(), AccessRights::READ_ADD))
    }

    /// Set refund purse.
    fn set_refund_purse(&mut self, purse: URef) -> Result<()> {
        internal::set_refund(self, purse)
    }

    /// Get refund purse.
    fn get_refund_purse(&self) -> Result<Option<URef>> {
        // We purposely choose to remove the access rights so that we do not
        // accidentally give rights for a purse to some contract that is not
        // supposed to have it.
        let maybe_purse = internal::get_refund_purse(self)?;
        Ok(maybe_purse.map(|p| p.remove_access_rights()))
    }

    /// Finalize payment with `amount_spent` and a given `account`.
    fn finalize_payment(&mut self, amount_spent: U512, account: AccountHash) -> Result<()> {
        internal::finalize_payment(self, amount_spent, account)
    }

    /// Mint and distribute seigniorage rewards to validators and their delegators,
    /// according to `reward_factors` returned by the consensus component.
    fn distribute(&mut self, reward_factors: BTreeMap<PublicKey, u64>) -> Result<()> {
        // let era_validators = self.read_winners();
        let seigniorage_recipients = self.read_seigniorage_recipients();
        let base_round_reward = U512::from(self.read_base_round_reward());

        // let unique_check_lhs: BTreeSet<&AccountHash> = reward_factors.keys().collect();
        // let unique_check_rhs: BTreeSet<&AccountHash> = seigniorage_recipients.keys().collect();

        if reward_factors.keys().ne(seigniorage_recipients.keys()) {
            return Err(Error::MismatchedEraValidators);
        }

        for (public_key, reward_factor) in reward_factors {
            let seigniorage_recipient = seigniorage_recipients.get(&public_key).unwrap();

            // Compute and mint rewards for the current validator
            let reward: U512 = base_round_reward * reward_factor;

            let total_stake = seigniorage_recipient.stake;
            // TODO: There should be no seigniorage recipient with 0 total_stake. Decide if the following is enough.
            if total_stake.is_zero() {
                continue;
            }

            let total_delegated_amount: U512 =
                seigniorage_recipient.delegations.values().cloned().sum();
            // Compute delegators' part
            let mut delegators_part: U512 = reward * total_delegated_amount / total_stake;
            let commission: U512 = delegators_part * seigniorage_recipient.commission_rate
                / COMMISSION_RATE_DENOMINATOR;
            delegators_part = delegators_part - commission;

            // Validator receives the rest
            let validators_part: U512 = reward - delegators_part;
            // Mint and transfer validator's part to the validator
            let tmp_purse = self.mint(validators_part);
            let reward_purse = internal::get_rewards_purse(self)?;
            self.transfer_purse_to_purse(tmp_purse, reward_purse, validators_part)
                .map_err(|_| Error::Transfer)?;
            let delegator_reward_purse = self.mint(delegators_part);
            // Distribute delegators' part
            self.distribute_to_delegators(public_key, delegator_reward_purse)?;
        }
        Ok(())
    }
}

mod internal {
    use alloc::vec::Vec;

    use crate::{
        account::AccountHash,
        system_contract_errors::pos::{Error, PurseLookupError, Result},
        BlockTime, Key, Phase, URef, U512,
    };

    use crate::proof_of_stake::{
        mint_provider::MintProvider, queue::QueueEntry, queue_provider::QueueProvider,
        runtime_provider::RuntimeProvider, stakes_provider::StakesProvider,
    };

    /// Account used to run system functions (in particular `finalize_payment`).
    const SYSTEM_ACCOUNT: AccountHash = AccountHash::new([0u8; 32]);

    /// The uref name where the PoS purse is stored. It contains all staked motes, and all unbonded
    /// motes that are yet to be paid out.
    const BONDING_PURSE_KEY: &str = "pos_bonding_purse";

    /// The uref name where the PoS accepts payment for computation on behalf of validators.
    const PAYMENT_PURSE_KEY: &str = "pos_payment_purse";

    /// The uref name where the PoS holds validator earnings before distributing them.
    const REWARDS_PURSE_KEY: &str = "pos_rewards_purse";

    /// The uref name where the PoS will refund unused payment back to the user. The uref this name
    /// corresponds to is set by the user.
    const REFUND_PURSE_KEY: &str = "pos_refund_purse";

    /// The time from a bonding request until the bond becomes effective and part of the stake.
    const BOND_DELAY: u64 = 0;

    /// The time from an unbonding request until the stakes are paid out.
    const UNBOND_DELAY: u64 = 0;

    /// The maximum number of pending bonding requests.
    const MAX_BOND_LEN: usize = 100;

    /// The maximum number of pending unbonding requests.
    const MAX_UNBOND_LEN: usize = 1000;

    /// Enqueues the deploy's creator for becoming a validator. The bond `amount` is paid from the
    /// purse `source`.
    pub fn bond<P: QueueProvider + StakesProvider>(
        provider: &mut P,
        amount: U512,
        validator: AccountHash,
        timestamp: BlockTime,
    ) -> Result<()> {
        let mut queue = provider.read_bonding();
        if queue.0.len() >= MAX_BOND_LEN {
            return Err(Error::TooManyEventsInQueue);
        }

        let mut stakes = provider.read()?;
        // Simulate applying all earlier bonds. The modified stakes are not written.
        for entry in &queue.0 {
            stakes.bond(&entry.validator, entry.amount);
        }
        stakes.validate_bonding(&validator, amount)?;

        queue.push(validator, amount, timestamp)?;
        provider.write_bonding(queue);
        Ok(())
    }

    /// Enqueues the deploy's creator for unbonding. Their vote weight as a validator is decreased
    /// immediately, but the funds will only be released after a delay. If `maybe_amount` is `None`,
    /// all funds are enqueued for withdrawal, terminating the validator status.
    pub fn unbond<P: QueueProvider + StakesProvider>(
        provider: &mut P,
        maybe_amount: Option<U512>,
        validator: AccountHash,
        timestamp: BlockTime,
    ) -> Result<()> {
        let mut queue = provider.read_unbonding();
        if queue.0.len() >= MAX_UNBOND_LEN {
            return Err(Error::TooManyEventsInQueue);
        }

        let mut stakes = provider.read()?;
        let payout = stakes.unbond(&validator, maybe_amount)?;
        provider.write(&stakes);
        // TODO: Make sure the destination is valid and the amount can be paid. The actual payment
        // will be made later, after the unbonding delay. contract_api::transfer_dry_run(POS_PURSE,
        // dest, amount)?;
        queue.push(validator, payout, timestamp)?;
        provider.write_unbonding(queue);
        Ok(())
    }

    /// Removes all due requests from the queues and applies them.
    pub fn step<P: QueueProvider + StakesProvider>(
        provider: &mut P,
        timestamp: BlockTime,
    ) -> Result<Vec<QueueEntry>> {
        let mut bonding_queue = provider.read_bonding();
        let mut unbonding_queue = provider.read_unbonding();

        let bonds = bonding_queue.pop_due(timestamp.saturating_sub(BlockTime::new(BOND_DELAY)));
        let unbonds =
            unbonding_queue.pop_due(timestamp.saturating_sub(BlockTime::new(UNBOND_DELAY)));

        if !unbonds.is_empty() {
            provider.write_unbonding(unbonding_queue);
        }

        if !bonds.is_empty() {
            provider.write_bonding(bonding_queue);
            let mut stakes = provider.read()?;
            for entry in bonds {
                stakes.bond(&entry.validator, entry.amount);
            }
            provider.write(&stakes);
        }

        Ok(unbonds)
    }

    /// Attempts to look up a purse from the named_keys
    fn get_purse<R: RuntimeProvider>(
        runtime_provider: &R,
        name: &str,
    ) -> core::result::Result<URef, PurseLookupError> {
        runtime_provider
            .get_key(name)
            .ok_or(PurseLookupError::KeyNotFound)
            .and_then(|key| match key {
                Key::URef(uref) => Ok(uref),
                _ => Err(PurseLookupError::KeyUnexpectedType),
            })
    }

    /// Returns the purse for accepting payment for transactions.
    pub fn get_payment_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<URef> {
        get_purse::<R>(runtime_provider, PAYMENT_PURSE_KEY).map_err(PurseLookupError::payment)
    }

    /// Returns the purse for holding bonds
    pub fn get_bonding_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<URef> {
        get_purse::<R>(runtime_provider, BONDING_PURSE_KEY).map_err(PurseLookupError::bonding)
    }

    /// Returns the purse for holding validator earnings
    pub fn get_rewards_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<URef> {
        get_purse::<R>(runtime_provider, REWARDS_PURSE_KEY).map_err(PurseLookupError::rewards)
    }

    /// Sets the purse where refunds (excess funds not spent to pay for computation) will be sent.
    /// Note that if this function is never called, the default location is the main purse of the
    /// deployer's account.
    pub fn set_refund<R: RuntimeProvider>(runtime_provider: &mut R, purse: URef) -> Result<()> {
        if let Phase::Payment = runtime_provider.get_phase() {
            runtime_provider.put_key(REFUND_PURSE_KEY, Key::URef(purse));
            return Ok(());
        }
        Err(Error::SetRefundPurseCalledOutsidePayment)
    }

    /// Returns the currently set refund purse.
    pub fn get_refund_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<Option<URef>> {
        match get_purse::<R>(runtime_provider, REFUND_PURSE_KEY) {
            Ok(uref) => Ok(Some(uref)),
            Err(PurseLookupError::KeyNotFound) => Ok(None),
            Err(PurseLookupError::KeyUnexpectedType) => Err(Error::RefundPurseKeyUnexpectedType),
        }
    }

    /// Transfers funds from the payment purse to the validator rewards purse, as well as to the
    /// refund purse, depending on how much was spent on the computation. This function maintains
    /// the invariant that the balance of the payment purse is zero at the beginning and end of each
    /// deploy and that the refund purse is unset at the beginning and end of each deploy.
    pub fn finalize_payment<P: MintProvider + RuntimeProvider>(
        provider: &mut P,
        amount_spent: U512,
        account: AccountHash,
    ) -> Result<()> {
        let caller = provider.get_caller();
        if caller != SYSTEM_ACCOUNT {
            return Err(Error::SystemFunctionCalledByUserAccount);
        }

        let payment_purse = get_payment_purse(provider)?;
        let total = match provider.balance(payment_purse) {
            Some(balance) => balance,
            None => return Err(Error::PaymentPurseBalanceNotFound),
        };

        if total < amount_spent {
            return Err(Error::InsufficientPaymentForAmountSpent);
        }
        let refund_amount = total - amount_spent;

        let rewards_purse = get_rewards_purse(provider)?;
        let refund_purse = get_refund_purse(provider)?;
        provider.remove_key(REFUND_PURSE_KEY); //unset refund purse after reading it

        // pay validators
        provider
            .transfer_purse_to_purse(payment_purse, rewards_purse, amount_spent)
            .map_err(|_| Error::FailedTransferToRewardsPurse)?;

        if refund_amount.is_zero() {
            return Ok(());
        }

        // give refund
        let refund_purse = match refund_purse {
            Some(uref) => uref,
            None => return refund_to_account::<P>(provider, payment_purse, account, refund_amount),
        };

        // in case of failure to transfer to refund purse we fall back on the account's main purse
        if provider
            .transfer_purse_to_purse(payment_purse, refund_purse, refund_amount)
            .is_err()
        {
            return refund_to_account::<P>(provider, payment_purse, account, refund_amount);
        }

        Ok(())
    }

    pub fn refund_to_account<M: MintProvider>(
        mint_provider: &mut M,
        payment_purse: URef,
        account: AccountHash,
        amount: U512,
    ) -> Result<()> {
        match mint_provider.transfer_purse_to_account(payment_purse, account, amount) {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::FailedTransferToAccountPurse),
        }
    }

    #[cfg(test)]
    mod tests {
        extern crate std;

        use std::{cell::RefCell, iter, thread_local};

        use crate::{account::AccountHash, system_contract_errors::pos::Result, BlockTime, U512};

        use super::{bond, step, unbond, BOND_DELAY, UNBOND_DELAY};
        use crate::proof_of_stake::{
            queue::Queue, queue_provider::QueueProvider, stakes::Stakes,
            stakes_provider::StakesProvider,
        };

        const KEY1: [u8; 32] = [1; 32];
        const KEY2: [u8; 32] = [2; 32];

        thread_local! {
            static BONDING: RefCell<Queue> = RefCell::new(Queue(Default::default()));
            static UNBONDING: RefCell<Queue> = RefCell::new(Queue(Default::default()));
            static STAKES: RefCell<Stakes> = RefCell::new(
                Stakes(iter::once((AccountHash::new(KEY1), U512::from(1_000))).collect())
            );
        }

        struct Provider;

        impl QueueProvider for Provider {
            fn read_bonding(&mut self) -> Queue {
                BONDING.with(|b| b.borrow().clone())
            }

            fn read_unbonding(&mut self) -> Queue {
                UNBONDING.with(|ub| ub.borrow().clone())
            }

            fn write_bonding(&mut self, queue: Queue) {
                BONDING.with(|b| b.replace(queue));
            }

            fn write_unbonding(&mut self, queue: Queue) {
                UNBONDING.with(|ub| ub.replace(queue));
            }
        }

        impl StakesProvider for Provider {
            fn read(&self) -> Result<Stakes> {
                STAKES.with(|s| Ok(s.borrow().clone()))
            }

            fn write(&mut self, stakes: &Stakes) {
                STAKES.with(|s| s.replace(stakes.clone()));
            }
        }

        fn assert_stakes(stakes: &[([u8; 32], usize)]) {
            let expected = Stakes(
                stakes
                    .iter()
                    .map(|(key, amount)| (AccountHash::new(*key), U512::from(*amount)))
                    .collect(),
            );
            assert_eq!(Ok(expected), Provider.read());
        }

        #[test]
        fn test_bond_step_unbond() {
            let mut provider = Provider;
            bond(
                &mut provider,
                U512::from(500),
                AccountHash::new(KEY2),
                BlockTime::new(1),
            )
            .expect("bond validator 2");

            // Bonding becomes effective only after the delay.
            assert_stakes(&[(KEY1, 1_000)]);
            step(&mut provider, BlockTime::new(BOND_DELAY)).expect("step 1");
            assert_stakes(&[(KEY1, 1_000)]);
            step(&mut provider, BlockTime::new(1 + BOND_DELAY)).expect("step 2");
            assert_stakes(&[(KEY1, 1_000), (KEY2, 500)]);

            unbond::<Provider>(
                &mut provider,
                Some(U512::from(500)),
                AccountHash::new(KEY1),
                BlockTime::new(2),
            )
            .expect("partly unbond validator 1");

            // Unbonding becomes effective immediately.
            assert_stakes(&[(KEY1, 500), (KEY2, 500)]);
            step::<Provider>(&mut provider, BlockTime::new(2 + UNBOND_DELAY)).expect("step 3");
            assert_stakes(&[(KEY1, 500), (KEY2, 500)]);
        }
    }
}
