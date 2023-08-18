mod error;

use super::{PricingMode, Transaction, TransactionKind};
use crate::{AccountAndSecretKey, PublicKey, SecretKey, TimeDiff, Timestamp};
pub use error::TransactionBuilderError;

/// A builder for constructing a [`Transaction`].
///
/// # Note
///
/// Before calling [`build`](Self::build), you must ensure that:
///   * an account is provided by either calling [`with_account`](Self::with_account) or
///     [`with_secret_key`](Self::with_secret_key)
///   * the chain name is set by calling [`with_chain_name`](Self::with_chain_name)
///   * the body is set by calling [`with_body`](Self::with_body)
///
/// If no secret key is provided, the resulting Transaction will be unsigned, and hence invalid.
/// It can be signed later (multiple times if desired) to make it valid before sending to the
/// network for execution.
pub struct TransactionBuilder<'a> {
    account: Option<PublicKey>,
    secret_key: Option<&'a SecretKey>,
    timestamp: Timestamp,
    ttl: TimeDiff,
    pricing_mode: PricingMode,
    chain_name: Option<String>,
    body: Option<TransactionKind>,
}

impl<'a> TransactionBuilder<'a> {
    /// The default time-to-live for `Transaction`s, i.e. 30 minutes.
    pub const DEFAULT_TTL: TimeDiff = TimeDiff::from_millis(30 * 60 * 1_000);
    /// The default pricing mode for `Transaction`s, i.e. multiplier of 1.
    pub const DEFAULT_PRICING_MODE: PricingMode = PricingMode::GasPriceMultiplier(1);

    /// Returns a new `TransactionBuilder`.
    pub fn new() -> Self {
        TransactionBuilder {
            account: None,
            secret_key: None,
            timestamp: Timestamp::now(),
            ttl: Self::DEFAULT_TTL,
            pricing_mode: Self::DEFAULT_PRICING_MODE,
            chain_name: None,
            body: None,
        }
    }

    /// Sets the `account` in the `Transaction`.
    ///
    /// If not provided, the public key derived from the secret key used in the `TransactionBuilder`
    /// will be used as the `account` in the `Transaction`.
    pub fn with_account(mut self, account: PublicKey) -> Self {
        self.account = Some(account);
        self
    }

    /// Sets the secret key used to sign the `Transaction` on calling [`build`](Self::build).
    ///
    /// If not provided, the `Transaction` can still be built, but will be unsigned and will be
    /// invalid until subsequently signed.
    pub fn with_secret_key(mut self, secret_key: &'a SecretKey) -> Self {
        self.secret_key = Some(secret_key);
        self
    }

    /// Sets the `timestamp` in the `Transaction`.
    ///
    /// If not provided, the timestamp will be set to the time when the `TransactionBuilder` was
    /// constructed.
    pub fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Sets the `ttl` (time-to-live) in the `Transaction`.
    ///
    /// If not provided, the ttl will be set to [`Self::DEFAULT_TTL`].
    pub fn with_ttl(mut self, ttl: TimeDiff) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the `chain_name` in the `Transaction`.
    ///
    /// Must be provided or building will fail.
    pub fn with_chain_name<C: Into<String>>(mut self, chain_name: C) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    /// Sets the `body` in the `Transaction`.
    ///
    /// Must be provided or building will fail.
    pub fn with_body(mut self, body: TransactionKind) -> Self {
        self.body = Some(body);
        self
    }

    /// Returns the new `Transaction`, or an error if non-defaulted fields were not set.
    ///
    /// For more info, see [the `TransactionBuilder` documentation](TransactionBuilder).
    pub fn build(self) -> Result<Transaction, TransactionBuilderError> {
        let account_and_secret_key = match (self.account, self.secret_key) {
            (Some(account), Some(secret_key)) => AccountAndSecretKey::Both {
                account,
                secret_key,
            },
            (Some(account), None) => AccountAndSecretKey::Account(account),
            (None, Some(secret_key)) => AccountAndSecretKey::SecretKey(secret_key),
            (None, None) => return Err(TransactionBuilderError::MissingAccount),
        };

        let chain_name = self
            .chain_name
            .ok_or(TransactionBuilderError::MissingChainName)?;
        let body = self.body.ok_or(TransactionBuilderError::MissingChainName)?;

        let transaction = Transaction::build(
            self.timestamp,
            self.ttl,
            self.pricing_mode,
            chain_name,
            body,
            account_and_secret_key,
        );
        Ok(transaction)
    }
}

impl<'a> Default for TransactionBuilder<'a> {
    fn default() -> Self {
        TransactionBuilder::new()
    }
}
