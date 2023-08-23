use rand::Rng;

use super::{PricingMode, Transaction, TransactionKind};
use crate::{
    testing::TestRng, AccountAndSecretKey, PublicKey, SecretKey, TimeDiff, Timestamp,
    TransactionApproval, TransactionConfig, TransactionHash,
};

/// A builder for constructing a random [`Transaction`] for use in tests.
pub struct TestTransactionBuilder {
    account: PublicKey,
    secret_key: Option<SecretKey>,
    timestamp: Timestamp,
    ttl: TimeDiff,
    pricing_mode: PricingMode,
    chain_name: String,
    body: TransactionKind,
    invalid_approvals: Vec<TransactionApproval>,
}

impl TestTransactionBuilder {
    /// Returns a new `TestTransactionBuilder` which will build a random, valid but possibly
    /// expired `Transaction`.
    ///
    /// The `Transaction` can be made invalid in the following ways:
    ///   * unsigned by calling `with_secret_key(None)`
    ///   * given an invalid approval by calling `with_invalid_approval`
    pub fn new(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        let ttl_millis = rng.gen_range(60_000..TransactionConfig::default().max_ttl.millis());
        TestTransactionBuilder {
            account: PublicKey::from(&secret_key),
            secret_key: Some(secret_key),
            timestamp: Timestamp::random(rng),
            ttl: TimeDiff::from_millis(ttl_millis),
            pricing_mode: PricingMode::random(rng),
            chain_name: rng.random_string(5..10),
            body: TransactionKind::random(rng),
            invalid_approvals: vec![],
        }
    }

    /// Sets the `account` in the `Transaction`.
    pub fn with_account(mut self, account: PublicKey) -> Self {
        self.account = account;
        self
    }

    /// Sets the secret key used to sign the `Transaction` on calling [`build`](Self::build).
    ///
    /// If set to `None`, the `Transaction` will be unsigned.
    pub fn with_secret_key(mut self, secret_key: Option<SecretKey>) -> Self {
        self.secret_key = secret_key;
        self
    }

    /// Sets the `timestamp` in the `Transaction`.
    pub fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Sets the `ttl` (time-to-live) in the `Transaction`.
    pub fn with_ttl(mut self, ttl: TimeDiff) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the `chain_name` in the `Transaction`.
    pub fn with_chain_name<C: Into<String>>(mut self, chain_name: C) -> Self {
        self.chain_name = chain_name.into();
        self
    }

    /// Sets the `body` in the `Transaction`.
    pub fn with_body(mut self, body: TransactionKind) -> Self {
        self.body = body;
        self
    }

    /// Sets an invalid approval in the `Transaction`.
    pub fn with_invalid_approval(mut self, rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        let hash = TransactionHash::random(rng);
        let approval = TransactionApproval::create(&hash, &secret_key);
        self.invalid_approvals.push(approval);
        self
    }

    /// Returns the new `Transaction`.
    pub fn build(self) -> Transaction {
        let account_and_secret_key = match &self.secret_key {
            Some(secret_key) => AccountAndSecretKey::Both {
                account: self.account,
                secret_key,
            },
            None => AccountAndSecretKey::Account(self.account),
        };

        let mut transaction = Transaction::build(
            self.timestamp,
            self.ttl,
            self.pricing_mode,
            self.chain_name,
            self.body,
            account_and_secret_key,
        );

        transaction.apply_approvals(self.invalid_approvals);

        transaction
    }
}
