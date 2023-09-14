use rand::Rng;

use super::{AccountAndSecretKey, PricingModeV1, TransactionV1, TransactionV1Kind};
use crate::{
    testing::TestRng, PublicKey, SecretKey, TimeDiff, Timestamp, TransactionConfig,
    TransactionV1Approval, TransactionV1Hash,
};

/// A builder for constructing a random [`TransactionV1`] for use in tests.
pub struct TestTransactionV1Builder {
    account: PublicKey,
    secret_key: Option<SecretKey>,
    timestamp: Timestamp,
    ttl: TimeDiff,
    pricing_mode: PricingModeV1,
    chain_name: String,
    payment: Option<u64>,
    body: TransactionV1Kind,
    invalid_approvals: Vec<TransactionV1Approval>,
}

impl TestTransactionV1Builder {
    /// Returns a new `TestTransactionV1Builder` which will build a random, valid but possibly
    /// expired transaction.
    ///
    /// The transaction can be made invalid in the following ways:
    ///   * unsigned by calling `with_secret_key(None)`
    ///   * given an invalid approval by calling `with_invalid_approval`
    pub fn new(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        let ttl_millis = rng.gen_range(60_000..TransactionConfig::default().max_ttl.millis());
        TestTransactionV1Builder {
            account: PublicKey::from(&secret_key),
            secret_key: Some(secret_key),
            timestamp: Timestamp::random(rng),
            ttl: TimeDiff::from_millis(ttl_millis),
            pricing_mode: PricingModeV1::random(rng),
            chain_name: rng.random_string(5..10),
            payment: Some(
                rng.gen_range(2_500_000_000..=TransactionConfig::default().block_gas_limit),
            ),
            body: TransactionV1Kind::random(rng),
            invalid_approvals: vec![],
        }
    }

    /// Sets the `account` in the transaction.
    pub fn with_account(mut self, account: PublicKey) -> Self {
        self.account = account;
        self
    }

    /// Sets the secret key used to sign the transaction on calling [`build`](Self::build).
    ///
    /// If set to `None`, the transaction will be unsigned.
    pub fn with_secret_key(mut self, secret_key: Option<SecretKey>) -> Self {
        self.secret_key = secret_key;
        self
    }

    /// Sets the `timestamp` in the transaction.
    pub fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Sets the `ttl` (time-to-live) in the transaction.
    pub fn with_ttl(mut self, ttl: TimeDiff) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sets the `chain_name` in the transaction.
    pub fn with_chain_name<C: Into<String>>(mut self, chain_name: C) -> Self {
        self.chain_name = chain_name.into();
        self
    }

    /// Sets the `payment` in the transaction.
    pub fn with_payment(mut self, payment: u64) -> Self {
        self.payment = Some(payment);
        self
    }

    /// Sets the `body` in the transaction.
    pub fn with_body(mut self, body: TransactionV1Kind) -> Self {
        self.body = body;
        self
    }

    /// Sets an invalid approval in the transaction.
    pub fn with_invalid_approval(mut self, rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        let hash = TransactionV1Hash::random(rng);
        let approval = TransactionV1Approval::create(&hash, &secret_key);
        self.invalid_approvals.push(approval);
        self
    }

    /// Returns the new transaction.
    pub fn build(self) -> TransactionV1 {
        let account_and_secret_key = match &self.secret_key {
            Some(secret_key) => AccountAndSecretKey::Both {
                account: self.account,
                secret_key,
            },
            None => AccountAndSecretKey::Account(self.account),
        };

        let mut transaction = TransactionV1::build(
            self.timestamp,
            self.ttl,
            self.pricing_mode,
            self.chain_name,
            self.payment,
            self.body,
            account_and_secret_key,
        );

        transaction.apply_approvals(self.invalid_approvals);

        transaction
    }
}
