mod account_and_secret_key;
mod error;

use super::{Deploy, DeployHash, ExecutableDeployItem, TransferTarget};
use crate::{PublicKey, SecretKey, TimeDiff, Timestamp, URef, U512};
pub(super) use account_and_secret_key::AccountAndSecretKey;
pub use error::DeployBuilderError;

/// A builder for constructing a [`Deploy`].
pub struct DeployBuilder<'a> {
    account: Option<PublicKey>,
    secret_key: Option<&'a SecretKey>,
    timestamp: Timestamp,
    ttl: TimeDiff,
    gas_price: u64,
    dependencies: Vec<DeployHash>,
    chain_name: String,
    payment: Option<ExecutableDeployItem>,
    session: ExecutableDeployItem,
}

impl<'a> DeployBuilder<'a> {
    /// The default time-to-live for `Deploy`s, i.e. 30 minutes.
    pub const DEFAULT_TTL: TimeDiff = TimeDiff::from_millis(30 * 60 * 1_000);
    /// The default gas price for `Deploy`s, i.e. `1`.
    pub const DEFAULT_GAS_PRICE: u64 = 1;

    /// Returns a new `DeployBuilder`.
    ///
    /// # Note
    ///
    /// Before calling [`build`](Self::build), you must ensure
    ///   * that an account is provided by either calling [`with_account`](Self::with_account) or
    ///     [`with_secret_key`](Self::with_secret_key)
    ///   * that payment code is provided by either calling
    ///     [`with_standard_payment`](Self::with_standard_payment) or
    ///     [`with_payment`](Self::with_payment)
    pub fn new<C: Into<String>>(chain_name: C, session: ExecutableDeployItem) -> Self {
        #[cfg(not(any(feature = "sdk")))]
        let timestamp = Timestamp::now();
        #[cfg(feature = "sdk")]
        let timestamp = Timestamp::zero();

        DeployBuilder {
            account: None,
            secret_key: None,
            timestamp,
            ttl: Self::DEFAULT_TTL,
            gas_price: Self::DEFAULT_GAS_PRICE,
            dependencies: vec![],
            chain_name: chain_name.into(),
            payment: None,
            session,
        }
    }

    /// Returns a new `DeployBuilder` with session code suitable for a transfer.
    ///
    /// If `maybe_source` is None, the account's main purse is used as the source of the transfer.
    ///
    /// # Note
    ///
    /// Before calling [`build`](Self::build), you must ensure
    ///   * that an account is provided by either calling [`with_account`](Self::with_account) or
    ///     [`with_secret_key`](Self::with_secret_key)
    ///   * that payment code is provided by either calling
    ///     [`with_standard_payment`](Self::with_standard_payment) or
    ///     [`with_payment`](Self::with_payment)
    pub fn new_transfer<C: Into<String>, A: Into<U512>>(
        chain_name: C,
        amount: A,
        maybe_source: Option<URef>,
        target: TransferTarget,
        maybe_transfer_id: Option<u64>,
    ) -> Self {
        let session =
            ExecutableDeployItem::new_transfer(amount, maybe_source, target, maybe_transfer_id);
        DeployBuilder::new(chain_name, session)
    }

    /// Sets the `account` in the `Deploy`.
    ///
    /// If not provided, the public key derived from the secret key used in the `DeployBuilder` will
    /// be used as the `account` in the `Deploy`.
    pub fn with_account(mut self, account: PublicKey) -> Self {
        self.account = Some(account);
        self
    }

    /// Sets the secret key used to sign the `Deploy` on calling [`build`](Self::build).
    ///
    /// If not provided, the `Deploy` can still be built, but will be unsigned and will be invalid
    /// until subsequently signed.
    pub fn with_secret_key(mut self, secret_key: &'a SecretKey) -> Self {
        self.secret_key = Some(secret_key);
        self
    }

    /// Sets the `payment` in the `Deploy` to a standard payment with the given amount.
    pub fn with_standard_payment<A: Into<U512>>(mut self, amount: A) -> Self {
        self.payment = Some(ExecutableDeployItem::new_standard_payment(amount));
        self
    }

    /// Sets the `payment` in the `Deploy`.
    pub fn with_payment(mut self, payment: ExecutableDeployItem) -> Self {
        self.payment = Some(payment);
        self
    }

    /// Sets the `timestamp` in the `Deploy`.
    ///
    /// If not provided, the timestamp will be set to the time when the `DeployBuilder` was
    /// constructed.
    pub fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Sets the `ttl` (time-to-live) in the `Deploy`.
    ///
    /// If not provided, the ttl will be set to [`Self::DEFAULT_TTL`].
    pub fn with_ttl(mut self, ttl: TimeDiff) -> Self {
        self.ttl = ttl;
        self
    }

    /// Returns the new `Deploy`, or an error if neither
    /// [`with_standard_payment`](Self::with_standard_payment) nor
    /// [`with_payment`](Self::with_payment) were previously called.
    pub fn build(self) -> Result<Deploy, DeployBuilderError> {
        let account_and_secret_key = match (self.account, self.secret_key) {
            (Some(account), Some(secret_key)) => AccountAndSecretKey::Both {
                account,
                secret_key,
            },
            (Some(account), None) => AccountAndSecretKey::Account(account),
            (None, Some(secret_key)) => AccountAndSecretKey::SecretKey(secret_key),
            (None, None) => return Err(DeployBuilderError::DeployMissingSessionAccount),
        };

        let payment = self
            .payment
            .ok_or(DeployBuilderError::DeployMissingPaymentCode)?;
        let deploy = Deploy::build(
            self.timestamp,
            self.ttl,
            self.gas_price,
            self.dependencies,
            self.chain_name,
            payment,
            self.session,
            account_and_secret_key,
        );
        Ok(deploy)
    }
}
