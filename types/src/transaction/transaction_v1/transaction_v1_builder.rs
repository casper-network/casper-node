mod error;

use super::{AccountAndSecretKey, PricingModeV1, TransactionV1, TransactionV1Kind};
use crate::{PublicKey, SecretKey, TimeDiff, Timestamp};
pub use error::TransactionV1BuilderError;

/// A builder for constructing a [`TransactionV1`].
///
/// # Note
///
/// Before calling [`build`](Self::build), you must ensure that:
///   * an account is provided by either calling [`with_account`](Self::with_account) or
///     [`with_secret_key`](Self::with_secret_key)
///   * the chain name is set by calling [`with_chain_name`](Self::with_chain_name)
///   * the body is set by calling [`with_body`](Self::with_body)
///
/// If no secret key is provided, the resulting transaction will be unsigned, and hence invalid.
/// It can be signed later (multiple times if desired) to make it valid before sending to the
/// network for execution.
pub struct TransactionV1Builder<'a> {
    account: Option<PublicKey>,
    secret_key: Option<&'a SecretKey>,
    timestamp: Timestamp,
    ttl: TimeDiff,
    pricing_mode: PricingModeV1,
    chain_name: Option<String>,
    payment: Option<u64>,
    body: Option<TransactionV1Kind>,
}

impl<'a> TransactionV1Builder<'a> {
    /// The default time-to-live for transactions, i.e. 30 minutes.
    pub const DEFAULT_TTL: TimeDiff = TimeDiff::from_millis(30 * 60 * 1_000);
    /// The default pricing mode for transactions, i.e. multiplier of 1.
    pub const DEFAULT_PRICING_MODE: PricingModeV1 = PricingModeV1::GasPriceMultiplier(1);

    /// Returns a new `TransactionV1Builder`.
    pub fn new() -> Self {
        TransactionV1Builder {
            account: None,
            secret_key: None,
            timestamp: Timestamp::now(),
            ttl: Self::DEFAULT_TTL,
            pricing_mode: Self::DEFAULT_PRICING_MODE,
            chain_name: None,
            payment: None,
            body: None,
        }
    }

    /// Sets the `account` in the transaction.
    ///
    /// If not provided, the public key derived from the secret key used in the builder will be
    /// used as the `account` in the transaction.
    pub fn with_account(mut self, account: PublicKey) -> Self {
        self.account = Some(account);
        self
    }

    /// Sets the secret key used to sign the transaction on calling [`build`](Self::build).
    ///
    /// If not provided, the transaction can still be built, but will be unsigned and will be
    /// invalid until subsequently signed.
    pub fn with_secret_key(mut self, secret_key: &'a SecretKey) -> Self {
        self.secret_key = Some(secret_key);
        self
    }

    /// Sets the `timestamp` in the transaction.
    ///
    /// If not provided, the timestamp will be set to the time when the builder was constructed.
    pub fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Sets the `ttl` (time-to-live) in the transaction.
    ///
    /// If not provided, the ttl will be set to [`Self::DEFAULT_TTL`].
    pub fn with_ttl(mut self, ttl: TimeDiff) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the `chain_name` in the transaction.
    ///
    /// Must be provided or building will fail.
    pub fn with_chain_name<C: Into<String>>(mut self, chain_name: C) -> Self {
        self.chain_name = Some(chain_name.into());
        self
    }

    /// Sets the `payment` in the transaction.
    ///
    /// If not provided, `payment` will be set to `None`.
    pub fn with_payment(mut self, payment: u64) -> Self {
        self.payment = Some(payment);
        self
    }

    /// Sets the `body` in the transaction.
    ///
    /// Must be provided or building will fail.
    pub fn with_body(mut self, body: TransactionV1Kind) -> Self {
        self.body = Some(body);
        self
    }

    /// Returns the new transaction, or an error if non-defaulted fields were not set.
    ///
    /// For more info, see [the `TransactionBuilder` documentation](TransactionV1Builder).
    pub fn build(self) -> Result<TransactionV1, TransactionV1BuilderError> {
        let account_and_secret_key = match (self.account, self.secret_key) {
            (Some(account), Some(secret_key)) => AccountAndSecretKey::Both {
                account,
                secret_key,
            },
            (Some(account), None) => AccountAndSecretKey::Account(account),
            (None, Some(secret_key)) => AccountAndSecretKey::SecretKey(secret_key),
            (None, None) => return Err(TransactionV1BuilderError::MissingAccount),
        };

        let chain_name = self
            .chain_name
            .ok_or(TransactionV1BuilderError::MissingChainName)?;
        let body = self
            .body
            .ok_or(TransactionV1BuilderError::MissingChainName)?;

        let transaction = TransactionV1::build(
            self.timestamp,
            self.ttl,
            self.pricing_mode,
            chain_name,
            self.payment,
            body,
            account_and_secret_key,
        );
        Ok(transaction)
    }
}

impl<'a> Default for TransactionV1Builder<'a> {
    fn default() -> Self {
        TransactionV1Builder::new()
    }
}
