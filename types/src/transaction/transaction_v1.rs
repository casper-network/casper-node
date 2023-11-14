mod errors_v1;
mod finalized_transaction_v1_approvals;
mod transaction_v1_approval;
mod transaction_v1_approvals_hash;
mod transaction_v1_body;
#[cfg(any(feature = "std", test))]
mod transaction_v1_builder;
mod transaction_v1_hash;
mod transaction_v1_header;

#[cfg(any(feature = "std", test))]
use alloc::string::ToString;
use alloc::{collections::BTreeSet, vec::Vec};
use core::{
    cmp,
    fmt::{self, Debug, Display, Formatter},
    hash,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};
use tracing::debug;

#[cfg(any(feature = "std", test))]
use super::InitiatorAddrAndSecretKey;
use super::{
    InitiatorAddr, PricingMode, TransactionEntryPoint, TransactionScheduling, TransactionTarget,
};
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
#[cfg(any(feature = "std", test))]
use crate::TransactionConfig;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    crypto, Digest, DisplayIter, RuntimeArgs, SecretKey, TimeDiff, Timestamp,
};
pub use errors_v1::{
    DecodeFromJsonErrorV1 as TransactionV1DecodeFromJsonError, ErrorV1 as TransactionV1Error,
    ExcessiveSizeErrorV1 as TransactionV1ExcessiveSizeError, TransactionV1ConfigFailure,
};
pub use finalized_transaction_v1_approvals::FinalizedTransactionV1Approvals;
pub use transaction_v1_approval::TransactionV1Approval;
pub use transaction_v1_approvals_hash::TransactionV1ApprovalsHash;
pub use transaction_v1_body::TransactionV1Body;
#[cfg(any(feature = "std", test))]
pub use transaction_v1_builder::{TransactionV1Builder, TransactionV1BuilderError};
pub use transaction_v1_hash::TransactionV1Hash;
pub use transaction_v1_header::TransactionV1Header;

/// A unit of work sent by a client to the network, which when executed can cause global state to
/// be altered.
///
/// To construct a new `TransactionV1`, use a [`TransactionV1Builder`].
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
    schemars(
        description = "A unit of work sent by a client to the network, which when executed can \
        cause global state to be altered."
    )
)]
pub struct TransactionV1 {
    hash: TransactionV1Hash,
    header: TransactionV1Header,
    body: TransactionV1Body,
    approvals: BTreeSet<TransactionV1Approval>,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    is_verified: OnceCell<Result<(), TransactionV1ConfigFailure>>,
}

impl TransactionV1 {
    /// Called by the `TransactionBuilder` to construct a new `TransactionV1`.
    #[cfg(any(feature = "std", test))]
    pub(super) fn build(
        chain_name: String,
        timestamp: Timestamp,
        ttl: TimeDiff,
        body: TransactionV1Body,
        pricing_mode: PricingMode,
        payment_amount: Option<u64>,
        initiator_addr_and_secret_key: InitiatorAddrAndSecretKey,
    ) -> TransactionV1 {
        let initiator_addr = initiator_addr_and_secret_key.initiator_addr();
        let body_hash = Digest::hash(
            body.to_bytes()
                .unwrap_or_else(|error| panic!("should serialize body: {}", error)),
        );
        let header = TransactionV1Header::new(
            chain_name,
            timestamp,
            ttl,
            body_hash,
            pricing_mode,
            payment_amount,
            initiator_addr,
        );

        let hash = header.compute_hash();
        let mut transaction = TransactionV1 {
            hash,
            header,
            body,
            approvals: BTreeSet::new(),
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        };

        if let Some(secret_key) = initiator_addr_and_secret_key.secret_key() {
            transaction.sign(secret_key);
        }
        transaction
    }

    /// Returns the hash identifying this transaction.
    pub fn hash(&self) -> &TransactionV1Hash {
        &self.hash
    }

    /// Returns the name of the chain the transaction should be executed on.
    pub fn chain_name(&self) -> &str {
        self.header.chain_name()
    }

    /// Returns the creation timestamp of the transaction.
    pub fn timestamp(&self) -> Timestamp {
        self.header.timestamp()
    }

    /// Returns the duration after the creation timestamp for which the transaction will stay valid.
    ///
    /// After this duration has ended, the transaction will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.header.ttl()
    }

    /// Returns `true` if the transaction has expired.
    pub fn expired(&self, current_instant: Timestamp) -> bool {
        self.header.expired(current_instant)
    }

    /// Returns the pricing mode for the transaction.
    pub fn pricing_mode(&self) -> &PricingMode {
        self.header.pricing_mode()
    }

    /// Returns the payment amount for the transaction.
    pub fn payment_amount(&self) -> Option<u64> {
        self.header.payment_amount()
    }

    /// Returns the address of the initiator of the transaction.
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        self.header.initiator_addr()
    }

    /// Returns a reference to the header of this transaction.
    pub fn header(&self) -> &TransactionV1Header {
        &self.header
    }

    /// Consumes `self`, returning the header of this transaction.
    pub fn take_header(self) -> TransactionV1Header {
        self.header
    }

    /// Returns the runtime args of the transaction.
    pub fn args(&self) -> &RuntimeArgs {
        self.body.args()
    }

    /// Returns the target of the transaction.
    pub fn target(&self) -> &TransactionTarget {
        self.body.target()
    }

    /// Returns the entry point of the transaction.
    pub fn entry_point(&self) -> &TransactionEntryPoint {
        self.body.entry_point()
    }

    /// Returns the scheduling kind of the transaction.
    pub fn scheduling(&self) -> &TransactionScheduling {
        self.body.scheduling()
    }

    /// Returns the body of this transaction.
    pub fn body(&self) -> &TransactionV1Body {
        &self.body
    }

    /// Returns the approvals for this transaction.
    pub fn approvals(&self) -> &BTreeSet<TransactionV1Approval> {
        &self.approvals
    }

    /// Adds a signature of this transaction's hash to its approvals.
    pub fn sign(&mut self, secret_key: &SecretKey) {
        let approval = TransactionV1Approval::create(&self.hash, secret_key);
        self.approvals.insert(approval);
    }

    /// Returns the `TransactionV1ApprovalsHash` of this transaction's approvals.
    pub fn compute_approvals_hash(&self) -> Result<TransactionV1ApprovalsHash, bytesrepr::Error> {
        TransactionV1ApprovalsHash::compute(&self.approvals)
    }

    /// Returns `true` if the serialized size of the transaction is not greater than
    /// `max_transaction_size`.
    #[cfg(any(feature = "std", test))]
    fn is_valid_size(
        &self,
        max_transaction_size: u32,
    ) -> Result<(), TransactionV1ExcessiveSizeError> {
        let actual_transaction_size = self.serialized_length();
        if actual_transaction_size > max_transaction_size as usize {
            return Err(TransactionV1ExcessiveSizeError {
                max_transaction_size,
                actual_transaction_size,
            });
        }
        Ok(())
    }

    /// Returns `Ok` if and only if this transaction's body hashes to the value of `body_hash()`,
    /// and if this transaction's header hashes to the value claimed as the transaction hash.
    pub fn has_valid_hash(&self) -> Result<(), TransactionV1ConfigFailure> {
        let body_hash = Digest::hash(
            self.body
                .to_bytes()
                .unwrap_or_else(|error| panic!("should serialize body: {}", error)),
        );
        if body_hash != *self.header.body_hash() {
            debug!(?self, ?body_hash, "invalid transaction body hash");
            return Err(TransactionV1ConfigFailure::InvalidBodyHash);
        }

        let hash = TransactionV1Hash::new(Digest::hash(
            self.header
                .to_bytes()
                .unwrap_or_else(|error| panic!("should serialize header: {}", error)),
        ));
        if hash != self.hash {
            debug!(?self, ?hash, "invalid transaction hash");
            return Err(TransactionV1ConfigFailure::InvalidTransactionHash);
        }
        Ok(())
    }

    /// Returns `Ok` if and only if:
    ///   * the transaction hash is correct (see [`TransactionV1::has_valid_hash`] for details)
    ///   * approvals are non empty, and
    ///   * all approvals are valid signatures of the signed hash
    pub fn verify(&self) -> Result<(), TransactionV1ConfigFailure> {
        #[cfg(any(feature = "once_cell", test))]
        return self.is_verified.get_or_init(|| self.do_verify()).clone();

        #[cfg(not(any(feature = "once_cell", test)))]
        self.do_verify()
    }

    fn do_verify(&self) -> Result<(), TransactionV1ConfigFailure> {
        if self.approvals.is_empty() {
            debug!(?self, "transaction has no approvals");
            return Err(TransactionV1ConfigFailure::EmptyApprovals);
        }

        self.has_valid_hash()?;

        for (index, approval) in self.approvals.iter().enumerate() {
            if let Err(error) = crypto::verify(self.hash, approval.signature(), approval.signer()) {
                debug!(
                    ?self,
                    "failed to verify transaction approval {}: {}", index, error
                );
                return Err(TransactionV1ConfigFailure::InvalidApproval { index, error });
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
        timestamp_leeway: TimeDiff,
        at: Timestamp,
    ) -> Result<(), TransactionV1ConfigFailure> {
        self.is_valid_size(config.max_transaction_size)?;

        let header = self.header();
        if header.chain_name() != chain_name {
            debug!(
                transaction_hash = %self.hash(),
                transaction_header = %header,
                chain_name = %header.chain_name(),
                "invalid chain identifier"
            );
            return Err(TransactionV1ConfigFailure::InvalidChainName {
                expected: chain_name.to_string(),
                got: header.chain_name().to_string(),
            });
        }

        header.is_valid(config, timestamp_leeway, at, &self.hash)?;

        if self.approvals.len() > max_associated_keys as usize {
            debug!(
                transaction_hash = %self.hash(),
                number_of_approvals = %self.approvals.len(),
                max_associated_keys = %max_associated_keys,
                "number of transaction approvals exceeds the limit"
            );
            return Err(TransactionV1ConfigFailure::ExcessiveApprovals {
                got: self.approvals.len() as u32,
                max_associated_keys,
            });
        }

        if let Some(payment) = self.payment_amount() {
            if payment > config.block_gas_limit {
                debug!(
                    amount = %payment,
                    block_gas_limit = %config.block_gas_limit,
                    "payment amount exceeds block gas limit"
                );
                return Err(TransactionV1ConfigFailure::ExceedsBlockGasLimit {
                    block_gas_limit: config.block_gas_limit,
                    got: payment,
                });
            }
        }

        self.body.is_valid(config)
    }

    // This method is not intended to be used by third party crates.
    //
    // It is required to allow finalized approvals to be injected after reading a transaction from
    // storage.
    #[doc(hidden)]
    pub fn with_approvals(mut self, approvals: BTreeSet<TransactionV1Approval>) -> Self {
        self.approvals = approvals;
        self
    }

    /// Returns a random, valid but possibly expired transaction.
    ///
    /// Note that the [`TransactionV1Builder`] can be used to create a random transaction with
    /// more specific values.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn random(rng: &mut TestRng) -> Self {
        TransactionV1Builder::new_random(rng).build().unwrap()
    }

    /// Turns `self` into an invalid transaction by clearing the `chain_name`, invalidating the
    /// transaction header hash.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub fn invalidate(&mut self) {
        self.header.invalidate();
    }

    /// Used by the `TestTransactionV1Builder` to inject invalid approvals for testing purposes.
    #[cfg(any(all(feature = "std", feature = "testing"), test))]
    pub(super) fn apply_approvals(&mut self, approvals: Vec<TransactionV1Approval>) {
        self.approvals.extend(approvals);
    }
}

impl hash::Hash for TransactionV1 {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
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

impl PartialEq for TransactionV1 {
    fn eq(&self, other: &TransactionV1) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
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

impl Ord for TransactionV1 {
    fn cmp(&self, other: &TransactionV1) -> cmp::Ordering {
        // Destructure to make sure we don't accidentally omit fields.
        let TransactionV1 {
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

impl PartialOrd for TransactionV1 {
    fn partial_cmp(&self, other: &TransactionV1) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ToBytes for TransactionV1 {
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

impl FromBytes for TransactionV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = TransactionV1Hash::from_bytes(bytes)?;
        let (header, remainder) = TransactionV1Header::from_bytes(remainder)?;
        let (body, remainder) = TransactionV1Body::from_bytes(remainder)?;
        let (approvals, remainder) = BTreeSet::<TransactionV1Approval>::from_bytes(remainder)?;
        let transaction = TransactionV1 {
            hash,
            header,
            body,
            approvals,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        };
        Ok((transaction, remainder))
    }
}

impl Display for TransactionV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "transaction-v1[{}, {}, approvals: {}]",
            self.header,
            self.body,
            DisplayIter::new(self.approvals.iter())
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const MAX_ASSOCIATED_KEYS: u32 = 5;

    #[test]
    fn json_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1::random(rng);
        let json_string = serde_json::to_string_pretty(&transaction).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(transaction, decoded);
    }

    #[test]
    fn bincode_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1::random(rng);
        let serialized = bincode::serialize(&transaction).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(transaction, deserialized);
    }

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1::random(rng);
        bytesrepr::test_serialization_roundtrip(transaction.header());
        bytesrepr::test_serialization_roundtrip(&transaction);
    }

    #[test]
    fn is_valid() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1::random(rng);
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
        invalid_transaction: TransactionV1,
        expected_error: TransactionV1ConfigFailure,
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
            TransactionV1ConfigFailure::InvalidApproval {
                index: expected_index,
                ..
            } => match actual_error {
                TransactionV1ConfigFailure::InvalidApproval {
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
        let mut transaction = TransactionV1::random(rng);

        transaction.invalidate();
        check_is_not_valid(
            transaction,
            TransactionV1ConfigFailure::InvalidTransactionHash,
        );
    }

    #[test]
    fn not_valid_due_to_empty_approvals() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1Builder::new_random(rng)
            .with_no_secret_key()
            .build()
            .unwrap();
        assert!(transaction.approvals.is_empty());
        check_is_not_valid(transaction, TransactionV1ConfigFailure::EmptyApprovals)
    }

    #[test]
    fn not_valid_due_to_invalid_approval() {
        let rng = &mut TestRng::new();
        let transaction = TransactionV1Builder::new_random(rng)
            .with_invalid_approval(rng)
            .build()
            .unwrap();

        // The expected index for the invalid approval will be the first index at which there is an
        // approval where the signer is not the account holder.
        let account_holder = match transaction.initiator_addr() {
            InitiatorAddr::PublicKey(public_key) => public_key.clone(),
            InitiatorAddr::AccountHash(_) | InitiatorAddr::EntityAddr(_) => unreachable!(),
        };
        let expected_index = transaction
            .approvals
            .iter()
            .enumerate()
            .find(|(_, approval)| approval.signer() != &account_holder)
            .map(|(index, _)| index)
            .unwrap();
        check_is_not_valid(
            transaction,
            TransactionV1ConfigFailure::InvalidApproval {
                index: expected_index,
                error: crypto::Error::SignatureError, // This field is ignored in the check.
            },
        );
    }

    #[test]
    fn is_config_compliant() {
        let rng = &mut TestRng::new();
        let chain_name = "net-1";
        let transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .build()
            .unwrap();

        let transaction_config = TransactionConfig::default();
        let current_timestamp = transaction.timestamp();
        transaction
            .is_config_compliant(
                chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
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

        let transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(wrong_chain_name)
            .build()
            .unwrap();

        let expected_error = TransactionV1ConfigFailure::InvalidChainName {
            expected: expected_chain_name.to_string(),
            got: wrong_chain_name.to_string(),
        };

        let current_timestamp = transaction.timestamp();
        assert_eq!(
            transaction.is_config_compliant(
                expected_chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
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
        let transaction = TransactionV1Builder::new_random(rng)
            .with_ttl(ttl)
            .with_chain_name(chain_name)
            .build()
            .unwrap();

        let expected_error = TransactionV1ConfigFailure::ExcessiveTimeToLive {
            max_ttl: transaction_config.max_ttl,
            got: ttl,
        };

        let current_timestamp = transaction.timestamp();
        assert_eq!(
            transaction.is_config_compliant(
                chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
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
        let leeway = TimeDiff::from_seconds(2);

        let transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .build()
            .unwrap();
        let current_timestamp = transaction.timestamp() - leeway - TimeDiff::from_seconds(1);

        let expected_error = TransactionV1ConfigFailure::TimestampInFuture {
            validation_timestamp: current_timestamp,
            timestamp_leeway: leeway,
            got: transaction.timestamp(),
        };

        assert_eq!(
            transaction.is_config_compliant(
                chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                leeway,
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
        let mut transaction = TransactionV1Builder::new_random(rng)
            .with_chain_name(chain_name)
            .build()
            .unwrap();

        for _ in 0..MAX_ASSOCIATED_KEYS {
            transaction.sign(&SecretKey::random(rng));
        }

        let current_timestamp = transaction.timestamp();

        let expected_error = TransactionV1ConfigFailure::ExcessiveApprovals {
            got: MAX_ASSOCIATED_KEYS + 1,
            max_associated_keys: MAX_ASSOCIATED_KEYS,
        };

        assert_eq!(
            transaction.is_config_compliant(
                chain_name,
                &transaction_config,
                MAX_ASSOCIATED_KEYS,
                TimeDiff::default(),
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
