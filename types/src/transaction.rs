mod auction_transaction;
mod direct_call;
mod error;
mod native_transaction;
mod pricing_mode;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
mod test_transaction_builder;
mod transaction_approval;
#[cfg(any(feature = "std", test))]
mod transaction_builder;
mod transaction_hash;
mod transaction_header;
mod transaction_kind;
mod userland_transaction;

use alloc::{collections::BTreeSet, vec::Vec};
use core::{
    cmp,
    fmt::{self, Debug, Display, Formatter},
    hash,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
use tracing::debug;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, Digest, DisplayIter, PublicKey, SecretKey, TimeDiff, Timestamp,
};
#[cfg(any(feature = "std", test))]
use crate::{AccountAndSecretKey, TransactionConfig};
pub use auction_transaction::AuctionTransaction;
pub use direct_call::DirectCall;
pub use error::{
    DecodeFromJsonError as TransactionDecodeFromJsonError, Error as TransactionError,
    ExcessiveSizeError as TransactionExcessiveSizeError, TransactionConfigFailure,
};
pub use native_transaction::NativeTransaction;
pub use pricing_mode::PricingMode;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
pub use test_transaction_builder::TestTransactionBuilder;
pub use transaction_approval::TransactionApproval;
#[cfg(any(feature = "std", test))]
pub use transaction_builder::{TransactionBuilder, TransactionBuilderError};
pub use transaction_hash::TransactionHash;
pub use transaction_header::TransactionHeader;
pub use transaction_kind::TransactionKind;
pub use userland_transaction::UserlandTransaction;

#[cfg(feature = "json-schema")]
static TRANSACTION: Lazy<Transaction> = Lazy::new(|| {
    let secret_key = SecretKey::example();
    let args = crate::runtime_args! { "amount" => 1000 };
    Transaction::build(
        *Timestamp::example(),
        TimeDiff::from_seconds(3_600),
        PricingMode::GasPriceMultiplier(1),
        String::from("casper-example"),
        TransactionKind::from(NativeTransaction::new_mint_transfer(args)),
        AccountAndSecretKey::SecretKey(secret_key),
    )
});

/// A signed smart contract.
///
/// To construct a new `Transaction`, use a [`TransactionBuilder`].
#[derive(Clone, Eq, Debug)]
#[cfg_attr(
    any(feature = "std", test),
    derive(Serialize, Deserialize),
    serde(deny_unknown_fields)
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A signed smart contract.")
)]
pub struct Transaction {
    hash: TransactionHash,
    header: TransactionHeader,
    body: TransactionKind,
    approvals: BTreeSet<TransactionApproval>,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    is_verified: OnceCell<Result<(), TransactionConfigFailure>>,
}

impl Transaction {
    /// Called by the `TransactionBuilder` to construct a new `Transaction`.
    #[cfg(any(feature = "std", test))]
    fn build(
        timestamp: Timestamp,
        ttl: TimeDiff,
        pricing_mode: PricingMode,
        chain_name: String,
        body: TransactionKind,
        account_and_secret_key: AccountAndSecretKey,
    ) -> Transaction {
        let account = account_and_secret_key.account();
        let header = TransactionHeader::new(account, timestamp, ttl, pricing_mode, chain_name);

        let mut transaction = Transaction {
            hash: TransactionHash::default(),
            header,
            body,
            approvals: BTreeSet::new(),
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        };

        if let Some(secret_key) = account_and_secret_key.secret_key() {
            transaction.sign(secret_key);
        }
        transaction
    }

    /// Returns the `TransactionHash` identifying this `Transaction`.
    pub fn hash(&self) -> &TransactionHash {
        &self.hash
    }

    /// Returns the public key of the account providing the context in which to run the
    /// `Transaction`.
    pub fn account(&self) -> &PublicKey {
        self.header.account()
    }

    /// Returns the creation timestamp of the `Transaction`.
    pub fn timestamp(&self) -> Timestamp {
        self.header.timestamp()
    }

    /// Returns the duration after the creation timestamp for which the `Transaction` will stay
    /// valid.
    ///
    /// After this duration has ended, the `Transaction` will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.header.ttl()
    }

    /// Returns `true` if the `Transaction` has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.header.expired(current_instant)
    }

    /// Returns the pricing mode for the `Transaction`.
    pub fn pricing_mode(&self) -> &PricingMode {
        self.header.pricing_mode()
    }

    /// Returns the name of the chain the `Transaction` should be executed on.
    pub fn chain_name(&self) -> &str {
        self.header.chain_name()
    }

    /// Returns a reference to the `TransactionHeader` of this `Transaction`.
    pub fn header(&self) -> &TransactionHeader {
        &self.header
    }

    /// Consumes `self`, returning the `TransactionHeader` of this `Transaction`.
    pub fn take_header(self) -> TransactionHeader {
        self.header
    }

    /// Returns the `TransactionKind` of this `Transaction`.
    pub fn body(&self) -> &TransactionKind {
        &self.body
    }

    /// Returns the `TransactionApproval`s for this `Transaction`.
    pub fn approvals(&self) -> &BTreeSet<TransactionApproval> {
        &self.approvals
    }

    /// Adds a signature of the hash of this `Transaction`'s header and body to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey) {
        let digest_to_sign = make_digest_to_sign(&self.header, &self.body);
        let approval = TransactionApproval::create(&digest_to_sign, secret_key);
        self.approvals.insert(approval);
        self.hash = self.calculate_hash(&digest_to_sign);
    }

    fn calculate_hash(&self, digest_to_sign: &Digest) -> TransactionHash {
        let mut buffer = Vec::with_capacity(
            digest_to_sign.serialized_length() + self.approvals.serialized_length(),
        );
        digest_to_sign
            .write_bytes(&mut buffer)
            .unwrap_or_else(|error| panic!("should serialize digest: {}", error));
        self.approvals
            .write_bytes(&mut buffer)
            .unwrap_or_else(|error| panic!("should serialize approvals: {}", error));
        TransactionHash::new(Digest::hash(buffer))
    }

    /// Returns `true` if the serialized size of the `Transaction` is not greater than
    /// `max_transaction_size`.
    #[cfg(any(feature = "std", test))]
    fn is_valid_size(
        &self,
        max_transaction_size: u32,
    ) -> Result<(), TransactionExcessiveSizeError> {
        let actual_transaction_size = self.serialized_length();
        if actual_transaction_size > max_transaction_size as usize {
            return Err(TransactionExcessiveSizeError {
                max_transaction_size,
                actual_transaction_size,
            });
        }
        Ok(())
    }

    /// Returns `Ok` if and only if the `Transaction` hash is correct.
    ///
    /// This should be the hash of the signed hash concatenated with the approvals, where the
    /// signed hash should be the hash of the header concatenated with the body.
    pub fn has_valid_hash(&self) -> Result<(), TransactionConfigFailure> {
        self.do_verify(true)
    }

    /// Returns `Ok` if and only if:
    ///   * the `Transaction` hash is correct (see [`Transaction::has_valid_hash`] for details)
    ///   * approvals are non empty, and
    ///   * all approvals are valid signatures of the signed hash
    pub fn verify(&self) -> Result<(), TransactionConfigFailure> {
        #[cfg(any(feature = "once_cell", test))]
        return self
            .is_verified
            .get_or_init(|| self.do_verify(false))
            .clone();

        #[cfg(not(any(feature = "once_cell", test)))]
        self.do_verify(false)
    }

    fn do_verify(&self, skip_approval_checks: bool) -> Result<(), TransactionConfigFailure> {
        if !skip_approval_checks && self.approvals.is_empty() {
            debug!(?self, "transaction has no approvals");
            return Err(TransactionConfigFailure::EmptyApprovals);
        }

        let digest_to_sign = make_digest_to_sign(&self.header, &self.body);
        let calculated_hash = self.calculate_hash(&digest_to_sign);
        if calculated_hash != self.hash {
            debug!(?self, ?calculated_hash, "invalid transaction hash");
            return Err(TransactionConfigFailure::InvalidTransactionHash);
        }

        if skip_approval_checks {
            return Ok(());
        }

        for (index, approval) in self.approvals.iter().enumerate() {
            if let Err(error) =
                crypto::verify(digest_to_sign, approval.signature(), approval.signer())
            {
                debug!(
                    ?self,
                    "failed to verify transaction approval {}: {}", index, error
                );
                return Err(TransactionConfigFailure::InvalidApproval { index, error });
            }
        }

        Ok(())
    }

    /// Returns `Ok` if and only if:
    ///   * the chain_name is correct,
    ///   * the configured parameters are complied with at the given timestamp
    #[cfg(any(feature = "std", test))]
    pub fn is_config_compliant(
        &self,
        chain_name: &str,
        config: &TransactionConfig,
        max_associated_keys: u32,
        at: Timestamp,
    ) -> Result<(), TransactionConfigFailure> {
        self.is_valid_size(config.max_transaction_size)?;

        let header = self.header();
        if header.chain_name() != chain_name {
            debug!(
                transaction_hash = %self.hash(),
                transaction_header = %header,
                chain_name = %header.chain_name(),
                "invalid chain identifier"
            );
            return Err(TransactionConfigFailure::InvalidChainName {
                expected: chain_name.to_string(),
                got: header.chain_name().to_string(),
            });
        }

        header.is_valid(config, at, &self.hash)?;

        if self.approvals.len() > max_associated_keys as usize {
            debug!(
                transaction_hash = %self.hash(),
                number_of_approvals = %self.approvals.len(),
                max_associated_keys = %max_associated_keys,
                "number of transaction approvals exceeds the limit"
            );
            return Err(TransactionConfigFailure::ExcessiveApprovals {
                got: self.approvals.len() as u32,
                max_associated_keys,
            });
        }

        let args_length = self.body.args().serialized_length();
        if args_length > config.max_args_length as usize {
            debug!(
                args_length,
                max_args_length = config.max_args_length,
                "transaction runtime args excessive size"
            );
            return Err(TransactionConfigFailure::ExcessiveArgsLength {
                max_length: config.max_args_length as usize,
                got: args_length,
            });
        }

        Ok(())
    }

    /// Returns a random, valid but possibly expired `Transaction`.
    ///
    /// Note that the [`TestTransactionBuilder`] can be used to create a random `Transaction` with
    /// more specific values.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        TestTransactionBuilder::new(rng).build()
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &TRANSACTION
    }

    /// Turns `self` into an invalid `Transaction` by clearing the `chain_name`, invalidating the
    /// `Transaction` header hash.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn invalidate(&mut self) {
        self.header.invalidate();
    }

    /// Used by the `TestTransactionBuilder` to inject invalid approvals for testing purposes.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(super) fn apply_approvals(&mut self, approvals: Vec<TransactionApproval>) {
        self.approvals.extend(approvals);
        let digest_to_sign = make_digest_to_sign(&self.header, &self.body);
        self.hash = self.calculate_hash(&digest_to_sign);
    }
}

impl hash::Hash for Transaction {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Destructure to make sure we don't accidentally omit fields.
        let Transaction {
            hash,
            header,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        hash.hash(state);
        header.hash(state);
        body.hash(state);
        approvals.hash(state);
    }
}

impl PartialEq for Transaction {
    fn eq(&self, other: &Transaction) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        let Transaction {
            hash,
            header,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        *hash == other.hash
            && *header == other.header
            && *body == other.body
            && *approvals == other.approvals
    }
}

impl Ord for Transaction {
    fn cmp(&self, other: &Transaction) -> cmp::Ordering {
        // Destructure to make sure we don't accidentally omit fields.
        let Transaction {
            hash,
            header,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
                is_verified: _,
        } = self;
        hash.cmp(&other.hash)
            .then_with(|| header.cmp(&other.header))
            .then_with(|| body.cmp(&other.body))
            .then_with(|| approvals.cmp(&other.approvals))
    }
}

impl PartialOrd for Transaction {
    fn partial_cmp(&self, other: &Transaction) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ToBytes for Transaction {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.hash.write_bytes(writer)?;
        self.header.write_bytes(writer)?;
        self.body.write_bytes(writer)?;
        self.approvals.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.hash.serialized_length()
            + self.header.serialized_length()
            + self.body.serialized_length()
            + self.approvals.serialized_length()
    }
}

impl FromBytes for Transaction {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = TransactionHash::from_bytes(bytes)?;
        let (header, remainder) = TransactionHeader::from_bytes(remainder)?;
        let (body, remainder) = TransactionKind::from_bytes(remainder)?;
        let (approvals, remainder) = BTreeSet::<TransactionApproval>::from_bytes(remainder)?;
        let transaction = Transaction {
            header,
            hash,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        };
        Ok((transaction, remainder))
    }
}

impl Display for Transaction {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction[{}, {}, body: {}, approvals: {}]",
            self.hash,
            self.header,
            self.body,
            DisplayIter::new(self.approvals.iter())
        )
    }
}

fn make_digest_to_sign(header: &TransactionHeader, body: &TransactionKind) -> Digest {
    let mut buffer = Vec::with_capacity(header.serialized_length() + body.serialized_length());
    header
        .write_bytes(&mut buffer)
        .unwrap_or_else(|error| panic!("should serialize transaction header: {}", error));
    body.write_bytes(&mut buffer)
        .unwrap_or_else(|error| panic!("should serialize transaction body: {}", error));
    Digest::hash(buffer)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const MAX_ASSOCIATED_KEYS: u32 = 5;

    #[test]
    fn json_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = Transaction::random(rng);
        let json_string = serde_json::to_string_pretty(&transaction).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(transaction, decoded);
    }

    #[test]
    fn bincode_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = Transaction::random(rng);
        let serialized = bincode::serialize(&transaction).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(transaction, deserialized);
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = Transaction::random(rng);
        bytesrepr::test_serialization_roundtrip(transaction.header());
        bytesrepr::test_serialization_roundtrip(&transaction);
    }

    #[test]
    fn is_valid() {
        let rng = &mut TestRng::new();
        let transaction = Transaction::random(rng);
        assert_eq!(
            transaction.is_verified.get(),
            None,
            "is_verified should initially be None"
        );
        transaction.verify().expect("should verify");
        assert_eq!(
            transaction.is_verified.get(),
            Some(&Ok(())),
            "is_verified should be true"
        );
    }

    fn check_is_not_valid(
        invalid_transaction: Transaction,
        expected_error: TransactionConfigFailure,
    ) {
        assert!(
            invalid_transaction.is_verified.get().is_none(),
            "is_verified should initially be None"
        );
        let actual_error = invalid_transaction.verify().unwrap_err();

        // Ignore the `error_msg` field of `InvalidApproval` when comparing to expected error, as
        // this makes the test too fragile.  Otherwise expect the actual error should exactly match
        // the expected error.
        match expected_error {
            TransactionConfigFailure::InvalidApproval {
                index: expected_index,
                ..
            } => match actual_error {
                TransactionConfigFailure::InvalidApproval {
                    index: actual_index,
                    ..
                } => {
                    assert_eq!(actual_index, expected_index);
                }
                _ => panic!("expected {}, got: {}", expected_error, actual_error),
            },
            _ => {
                assert_eq!(actual_error, expected_error,);
            }
        }

        // The actual error should have been lazily initialized correctly.
        assert_eq!(
            invalid_transaction.is_verified.get(),
            Some(&Err(actual_error)),
            "is_verified should now be Some"
        );
    }

    #[test]
    fn not_valid_due_to_invalid_transaction_hash() {
        let rng = &mut TestRng::new();
        let mut transaction = Transaction::random(rng);

        transaction.invalidate();
        check_is_not_valid(
            transaction,
            TransactionConfigFailure::InvalidTransactionHash,
        );
    }

    #[test]
    fn not_valid_due_to_empty_approvals() {
        let rng = &mut TestRng::new();
        let transaction = TestTransactionBuilder::new(rng)
            .with_secret_key(None)
            .build();
        assert!(transaction.approvals.is_empty());
        check_is_not_valid(transaction, TransactionConfigFailure::EmptyApprovals)
    }

    #[test]
    fn not_valid_due_to_invalid_approval() {
        let rng = &mut TestRng::new();
        let transaction = TestTransactionBuilder::new(rng)
            .with_invalid_approval(rng)
            .build();

        // The expected index for the invalid approval will be the first index at which there is an
        // approval where the signer is not the account holder.
        let account_holder = transaction.account();
        let expected_index = transaction
            .approvals
            .iter()
            .enumerate()
            .find(|(_, approval)| approval.signer() != account_holder)
            .map(|(index, _)| index)
            .unwrap();
        check_is_not_valid(
            transaction,
            TransactionConfigFailure::InvalidApproval {
                index: expected_index,
                error: crypto::Error::SignatureError, // This field is ignored in the check.
            },
        );
    }

    #[test]
    fn is_config_compliant() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let transaction = TestTransactionBuilder::new(rng)
            .with_chain_name(chain_name)
            .build();

        let transaction_config = TransactionConfig::default();
        let current_timestamp = transaction.timestamp();
        transaction
            .is_config_compliant(
                chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                current_timestamp,
            )
            .expect("should be acceptable");
    }

    #[test]
    fn not_acceptable_due_to_invalid_chain_name() {
        let rng = &mut TestRng::new();
        let expected_chain_name = "net-1";
        let wrong_chain_name = "net-2";
        let transaction_config = TransactionConfig::default();

        let transaction = TestTransactionBuilder::new(rng)
            .with_chain_name(wrong_chain_name)
            .build();

        let expected_error = TransactionConfigFailure::InvalidChainName {
            expected: expected_chain_name.to_string(),
            got: wrong_chain_name.to_string(),
        };

        let current_timestamp = transaction.timestamp();
        assert_eq!(
            transaction.is_config_compliant(
                expected_chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                current_timestamp
            ),
            Err(expected_error)
        );
        assert!(
            transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_ttl() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let transaction_config = TransactionConfig::default();
        let ttl = transaction_config.max_ttl + TimeDiff::from(Duration::from_secs(1));
        let transaction = TestTransactionBuilder::new(rng)
            .with_ttl(ttl)
            .with_chain_name(chain_name)
            .build();

        let expected_error = TransactionConfigFailure::ExcessiveTimeToLive {
            max_ttl: transaction_config.max_ttl,
            got: ttl,
        };

        let current_timestamp = transaction.timestamp();
        assert_eq!(
            transaction.is_config_compliant(
                chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                current_timestamp
            ),
            Err(expected_error)
        );
        assert!(
            transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_timestamp_in_future() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let transaction_config = TransactionConfig::default();
        let transaction = TestTransactionBuilder::new(rng)
            .with_chain_name(chain_name)
            .build();

        let current_timestamp = transaction.timestamp() - TimeDiff::from_seconds(1);

        let expected_error = TransactionConfigFailure::TimestampInFuture {
            validation_timestamp: current_timestamp,
            got: transaction.timestamp(),
        };

        assert_eq!(
            transaction.is_config_compliant(
                chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                current_timestamp
            ),
            Err(expected_error)
        );
        assert!(
            transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );
    }

    #[test]
    fn not_acceptable_due_to_excessive_approvals() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let transaction_config = TransactionConfig::default();
        let mut transaction = TestTransactionBuilder::new(rng)
            .with_chain_name(chain_name)
            .build();

        for _ in 0..MAX_ASSOCIATED_KEYS {
            transaction.sign(&SecretKey::random(rng));
        }

        let current_timestamp = transaction.timestamp();

        let expected_error = TransactionConfigFailure::ExcessiveApprovals {
            got: MAX_ASSOCIATED_KEYS + 1,
            max_associated_keys: MAX_ASSOCIATED_KEYS,
        };

        assert_eq!(
            transaction.is_config_compliant(
                chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                current_timestamp
            ),
            Err(expected_error)
        );
        assert!(
            transaction.is_verified.get().is_none(),
            "transaction should not have run expensive `is_verified` call"
        );
    }
}
