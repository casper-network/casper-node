//! The account nonces an EVM transaction consumes.
//!
//! An EVM transaction carries no timestamp or TTL which its hash commits to, so its only replay
//! protection is its sender's nonce: once a block including it has executed, the sender's nonce is
//! past the transaction's and the transaction can never be included again. An EIP-7702
//! transaction also consumes, for each authorization in its list, a nonce of the authorizing
//! account.
//!
//! Ethereum consumes the sender's nonce first and then each authorization's in list order, so an
//! account's nonces in one transaction must be consecutive, and an authorization signed by the
//! sender must continue from the transaction's own nonce. The nonces a transaction consumes are
//! therefore one span of consecutive nonces per account, and the transaction *reserves* those
//! spans while it is in flight: from being proposed until the block including it has executed.
//!
//! For example a transaction from `S1` with nonce 1 whose authorization list is
//! `[(2, S1), (1, S2), (2, S3), (3, S1), (3, S3), (4, S1), (1, S5)]` reserves `S1: 1..=4`,
//! `S2: 1..=1`, `S3: 2..=3` and `S5: 1..=1`.

use std::{
    collections::{btree_map, BTreeMap},
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{evm, EvmTransaction, EvmTransactionError};

/// A span of consecutive nonces of one account, both ends inclusive.
#[derive(Clone, Copy, Debug, DataSize, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct NonceSpan {
    first: u64,
    last: u64,
}

impl NonceSpan {
    fn single(nonce: u64) -> Self {
        NonceSpan {
            first: nonce,
            last: nonce,
        }
    }

    /// The first nonce in the span: the account's nonce before the transaction executes.
    pub(crate) fn first(&self) -> u64 {
        self.first
    }

    /// The last nonce in the span.
    pub(crate) fn last(&self) -> u64 {
        self.last
    }

    /// The account's nonce after the transaction executes, or `None` on overflow.
    pub(crate) fn next(&self) -> Option<u64> {
        self.last.checked_add(1)
    }
}

impl Display for NonceSpan {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}..={}", self.first, self.last)
    }
}

/// The nonces an EVM transaction consumes: one span of consecutive nonces per account, starting
/// with its sender's.
#[derive(Clone, Debug, DataSize, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct EvmNonceReservation {
    sender: evm::Address,
    spans: BTreeMap<evm::Address, NonceSpan>,
}

impl EvmNonceReservation {
    /// Returns the nonces `transaction` consumes.
    ///
    /// Fails if an authorization's authority cannot be recovered, or if an account's nonces in the
    /// transaction are not consecutive. Either would make execution skip the authorization, so the
    /// transaction would not consume the nonces it claims.
    pub(crate) fn new(transaction: &EvmTransaction) -> Result<Self, EvmTransactionError> {
        let sender = transaction.from();
        let mut spans = BTreeMap::new();
        let _ = spans.insert(sender, NonceSpan::single(transaction.nonce()));
        for (index, authorization) in transaction.authorization_list().iter().enumerate() {
            let authority = authorization.recover_authority().map_err(|reason| {
                EvmTransactionError::InvalidAuthorizationSignature { index, reason }
            })?;
            match spans.entry(authority) {
                btree_map::Entry::Vacant(entry) => {
                    let _ = entry.insert(NonceSpan::single(authorization.nonce));
                }
                btree_map::Entry::Occupied(mut entry) => {
                    let span = entry.get_mut();
                    let expected = span.next().ok_or(
                        EvmTransactionError::NonConsecutiveAuthorizationNonce {
                            authority,
                            expected: u64::MAX,
                            actual: authorization.nonce,
                        },
                    )?;
                    if authorization.nonce != expected {
                        return Err(EvmTransactionError::NonConsecutiveAuthorizationNonce {
                            authority,
                            expected,
                            actual: authorization.nonce,
                        });
                    }
                    span.last = expected;
                }
            }
        }
        if spans.values().any(|span| span.next().is_none()) {
            // The account's nonce would overflow; execution rejects such a transaction.
            return Err(EvmTransactionError::Validation(
                "EVM nonce would overflow".to_string(),
            ));
        }
        Ok(EvmNonceReservation { sender, spans })
    }

    /// The transaction's sender.
    pub(crate) fn sender(&self) -> evm::Address {
        self.sender
    }

    /// The transaction's own nonce.
    pub(crate) fn sender_nonce(&self) -> u64 {
        self.spans
            .get(&self.sender)
            .map(NonceSpan::first)
            .unwrap_or_default()
    }

    /// The span of nonces reserved for each account.
    pub(crate) fn spans(&self) -> impl Iterator<Item = (&evm::Address, &NonceSpan)> {
        self.spans.iter()
    }

    /// The accounts whose nonces are reserved.
    pub(crate) fn accounts(&self) -> impl Iterator<Item = &evm::Address> {
        self.spans.keys()
    }

    /// Returns a reservation of the given spans, without checking them against a transaction.
    #[cfg(test)]
    pub(crate) fn from_spans(
        sender: evm::Address,
        spans: impl IntoIterator<Item = (evm::Address, u64, u64)>,
    ) -> Self {
        let spans: BTreeMap<_, _> = spans
            .into_iter()
            .map(|(account, first, last)| (account, NonceSpan { first, last }))
            .collect();
        assert!(spans.contains_key(&sender), "sender must have a span");
        EvmNonceReservation { sender, spans }
    }
}

impl Display for EvmNonceReservation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "nonces of sender {}:", self.sender)?;
        for (account, span) in &self.spans {
            write!(formatter, " {account}: {span}")?;
        }
        Ok(())
    }
}

/// An account's nonce does not match the one its reservation starts at.
#[derive(Clone, Copy, Debug, DataSize, Eq, PartialEq, Serialize)]
pub(crate) struct NonceMismatch {
    /// The account.
    pub(crate) account: evm::Address,
    /// The account's nonce at this point of the block.
    pub(crate) expected: u64,
    /// The nonce the reservation starts at.
    pub(crate) actual: u64,
}

impl Display for NonceMismatch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "EVM nonce of {} is {} at this point, but a transaction consumes it from {}",
            self.account, self.expected, self.actual
        )
    }
}

/// The nonces of the accounts reserved by a sequence of EVM transactions in one block, starting
/// from their nonces in the state the block builds on.
///
/// This is the single rule shared by the proposer and the block validator: each reservation must
/// start exactly at its accounts' nonces at that point of the block, so a block's EVM transactions
/// consume their accounts' nonces consecutively and in block order.
#[derive(Debug, Default)]
pub(crate) struct NonceChain {
    next: BTreeMap<evm::Address, u64>,
}

impl NonceChain {
    /// Checks that `reservation` starts at its accounts' nonces at this point of the block and, if
    /// it does, advances them past it.
    ///
    /// `state_nonce` returns an account's nonce in the state the block builds on; it is only called
    /// for accounts no earlier reservation in the block covers.
    pub(crate) fn try_extend(
        &mut self,
        reservation: &EvmNonceReservation,
        state_nonce: impl FnMut(&evm::Address) -> u64,
    ) -> Result<(), NonceMismatch> {
        self.check(reservation, state_nonce)?;
        self.advance(reservation);
        Ok(())
    }

    /// Checks that `reservation` starts at its accounts' nonces at this point of the block, without
    /// advancing them.
    pub(crate) fn check(
        &self,
        reservation: &EvmNonceReservation,
        mut state_nonce: impl FnMut(&evm::Address) -> u64,
    ) -> Result<(), NonceMismatch> {
        for (account, span) in reservation.spans() {
            let expected = match self.next.get(account) {
                Some(next) => *next,
                None => state_nonce(account),
            };
            if span.first() != expected {
                return Err(NonceMismatch {
                    account: *account,
                    expected,
                    actual: span.first(),
                });
            }
        }
        Ok(())
    }

    /// Returns whether an earlier reservation in the block covers `account`, so that its nonce at
    /// this point of the block doesn't depend on the state the block builds on.
    pub(crate) fn covers(&self, account: &evm::Address) -> bool {
        self.next.contains_key(account)
    }

    /// Advances the nonces of `reservation`'s accounts past it.
    pub(crate) fn advance(&mut self, reservation: &EvmNonceReservation) {
        for (account, span) in reservation.spans() {
            // `EvmNonceReservation::new` rejects spans whose end would overflow.
            let _ = self
                .next
                .insert(*account, span.next().unwrap_or(span.last()));
        }
    }
}

/// Signed EVM transactions for tests.
#[cfg(test)]
pub(crate) mod test_utils {
    use alloy_consensus::{SignableTransaction, TxEip7702, TxEnvelope, TxLegacy};
    use alloy_eips::{eip7702::Authorization, Encodable2718};
    use alloy_primitives::{Address as AlloyAddress, Bytes, Signature, TxKind, U256};
    use k256::ecdsa::{signature::hazmat::PrehashSigner, SigningKey};

    use casper_types::{evm, EvmConfig, EvmTransaction};

    fn signing_key(key: u8) -> SigningKey {
        SigningKey::from_slice(&[key; 32]).expect("valid key")
    }

    fn sign(key: u8, prehash: &[u8]) -> Signature {
        let (signature, recovery_id) = signing_key(key).sign_prehash(prehash).expect("signs");
        Signature::from((signature, recovery_id))
    }

    /// The address of the account signing with the test key `key`.
    pub(crate) fn evm_address(key: u8) -> evm::Address {
        let address = AlloyAddress::from_private_key(&signing_key(key));
        evm::Address::new(address.into_array())
    }

    /// A transaction signed with the test key `key`, with a `salt` in its input so that
    /// transactions which differ only by it have distinct hashes.
    ///
    /// Without authorizations it is a legacy transaction, otherwise an EIP-7702 transaction with
    /// an authorization signed with test key `authority` and nonce `nonce` for each `(authority,
    /// nonce)`.
    pub(crate) fn signed_evm_transaction(
        key: u8,
        nonce: u64,
        authorizations: &[(u8, u64)],
        salt: u64,
    ) -> EvmTransaction {
        let evm_config = EvmConfig::default();
        signed_evm_transaction_for(
            evm_config.chain_id,
            evm_config.base_fee_wei(),
            key,
            nonce,
            authorizations,
            salt,
        )
    }

    /// Like [`signed_evm_transaction`], for the given chain and gas price.
    pub(crate) fn signed_evm_transaction_for(
        chain_id: u64,
        gas_price: u128,
        key: u8,
        nonce: u64,
        authorizations: &[(u8, u64)],
        salt: u64,
    ) -> EvmTransaction {
        let input = Bytes::from(salt.to_le_bytes().to_vec());
        let envelope = if authorizations.is_empty() {
            let transaction = TxLegacy {
                chain_id: Some(chain_id),
                nonce,
                gas_price,
                gas_limit: 21_000,
                to: TxKind::Call([1; 20].into()),
                value: U256::ZERO,
                input,
            };
            let signature = sign(key, transaction.signature_hash().as_ref());
            TxEnvelope::from(transaction.into_signed(signature))
        } else {
            let authorization_list = authorizations
                .iter()
                .map(|(authority, nonce)| {
                    let authorization = Authorization {
                        chain_id: U256::from(chain_id),
                        address: [2; 20].into(),
                        nonce: *nonce,
                    };
                    let signature = sign(*authority, authorization.signature_hash().as_ref());
                    authorization.into_signed(signature)
                })
                .collect();
            let transaction = TxEip7702 {
                chain_id,
                nonce,
                gas_limit: 100_000,
                max_fee_per_gas: gas_price,
                max_priority_fee_per_gas: 0,
                to: [1; 20].into(),
                value: U256::ZERO,
                access_list: Default::default(),
                authorization_list,
                input,
            };
            let signature = sign(key, transaction.signature_hash().as_ref());
            TxEnvelope::from(transaction.into_signed(signature))
        };
        EvmTransaction::from_signed_rlp(envelope.encoded_2718()).expect("valid EVM transaction")
    }
}

#[cfg(test)]
mod tests {
    use super::{test_utils::*, *};

    fn address(byte: u8) -> evm::Address {
        evm::Address::new([byte; 20])
    }

    fn spans_of(reservation: &EvmNonceReservation) -> Vec<(evm::Address, u64, u64)> {
        reservation
            .spans()
            .map(|(account, span)| (*account, span.first(), span.last()))
            .collect()
    }

    #[test]
    fn transaction_without_authorizations_reserves_its_sender_nonce() {
        let transaction = signed_evm_transaction(1, 7, &[], 0);
        let reservation = EvmNonceReservation::new(&transaction).unwrap();
        assert_eq!(reservation.sender(), evm_address(1));
        assert_eq!(reservation.sender_nonce(), 7);
        assert_eq!(spans_of(&reservation), vec![(evm_address(1), 7, 7)]);
    }

    #[test]
    fn authorizations_reserve_a_span_per_account() {
        // (1, S1) with authorizations [(2, S1), (1, S2), (2, S3), (3, S1), (3, S3), (4, S1),
        // (1, S5)].
        let transaction = signed_evm_transaction(
            1,
            1,
            &[(1, 2), (2, 1), (3, 2), (1, 3), (3, 3), (1, 4), (5, 1)],
            0,
        );
        let reservation = EvmNonceReservation::new(&transaction).unwrap();
        let mut expected = vec![
            (evm_address(1), 1, 4),
            (evm_address(2), 1, 1),
            (evm_address(3), 2, 3),
            (evm_address(5), 1, 1),
        ];
        expected.sort();
        assert_eq!(spans_of(&reservation), expected);
        assert_eq!(reservation.sender_nonce(), 1);
    }

    #[test]
    fn authorization_by_sender_must_follow_the_transaction_nonce() {
        // The sender's nonce is consumed before its authorizations are applied, so its own
        // authorization reusing the transaction's nonce would be skipped.
        let transaction = signed_evm_transaction(1, 1, &[(1, 1)], 0);
        assert_eq!(
            EvmNonceReservation::new(&transaction),
            Err(EvmTransactionError::NonConsecutiveAuthorizationNonce {
                authority: evm_address(1),
                expected: 2,
                actual: 1,
            })
        );
    }

    #[test]
    fn nonce_which_would_overflow_is_rejected() {
        // Executing the transaction could not consume its nonce.
        let transaction = signed_evm_transaction(1, u64::MAX, &[], 0);
        assert!(matches!(
            EvmNonceReservation::new(&transaction),
            Err(EvmTransactionError::Validation(_))
        ));
    }

    #[test]
    fn authorization_nonces_of_an_account_must_be_consecutive() {
        let transaction = signed_evm_transaction(1, 0, &[(2, 1), (2, 3)], 0);
        assert_eq!(
            EvmNonceReservation::new(&transaction),
            Err(EvmTransactionError::NonConsecutiveAuthorizationNonce {
                authority: evm_address(2),
                expected: 2,
                actual: 3,
            })
        );
    }

    #[test]
    fn chain_requires_each_reservation_to_start_at_the_current_nonce() {
        let (s1, s2) = (address(1), address(2));
        let state = |account: &evm::Address| if *account == s1 { 1 } else { 7 };
        let mut chain = NonceChain::default();
        chain
            .try_extend(
                &EvmNonceReservation::from_spans(s1, [(s1, 1, 4), (s2, 7, 7)]),
                state,
            )
            .unwrap();
        // The next reservation of `s1` must continue after 4, whatever the state says.
        assert_eq!(
            chain.try_extend(&EvmNonceReservation::from_spans(s1, [(s1, 1, 1)]), state),
            Err(NonceMismatch {
                account: s1,
                expected: 5,
                actual: 1
            })
        );
        chain
            .try_extend(
                &EvmNonceReservation::from_spans(s2, [(s2, 8, 8), (s1, 5, 6)]),
                state,
            )
            .unwrap();
    }

    #[test]
    fn failed_extension_does_not_advance_the_chain() {
        let (s1, s2) = (address(1), address(2));
        let state = |_: &evm::Address| 0;
        let mut chain = NonceChain::default();
        assert!(chain
            .try_extend(
                &EvmNonceReservation::from_spans(s1, [(s1, 0, 0), (s2, 3, 3)]),
                state,
            )
            .is_err());
        chain
            .try_extend(&EvmNonceReservation::from_spans(s1, [(s1, 0, 0)]), state)
            .unwrap();
    }
}
