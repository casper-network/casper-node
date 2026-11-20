//! The EVM nonces reserved by transactions which are in flight.

use std::collections::{BTreeSet, HashMap};

use datasize::DataSize;

use casper_types::{evm, Timestamp, TransactionHash};

use crate::types::EvmNonceReservation;

#[derive(Debug, DataSize)]
struct InFlight {
    reservation: EvmNonceReservation,
    /// The timestamp of the latest proposal which included the transaction.
    latest_proposal: Timestamp,
    /// Whether a finalized block includes the transaction. The reservation is then held until that
    /// block has executed.
    finalized: bool,
}

/// The nonces reserved by EVM transactions this node considers in flight: from being included in
/// a proposal this node made or accepted, until the block including them has executed.
///
/// An EVM transaction's nonces can only be checked against executed state, so while a transaction
/// reserving an account's nonces is in flight, this node neither proposes nor accepts another
/// transaction reserving any nonce of that account. A proposal colliding with a reservation is
/// rejected without blaming its proposer, since whether a transaction is in flight is this node's
/// own view.
///
/// A reservation for a proposal which is not finalized lapses once a block with a later timestamp
/// is finalized, as the proposal was then abandoned.
#[derive(Debug, Default, DataSize)]
pub(crate) struct EvmReservations {
    by_transaction: HashMap<TransactionHash, InFlight>,
    #[data_size(skip)]
    by_account: HashMap<evm::Address, BTreeSet<TransactionHash>>,
}

impl EvmReservations {
    /// Records that `transaction_hash` is included in a proposal with timestamp
    /// `proposal_timestamp`, and reserves its nonces.
    pub(crate) fn reserve(
        &mut self,
        transaction_hash: TransactionHash,
        reservation: &EvmNonceReservation,
        proposal_timestamp: Timestamp,
        finalized: bool,
    ) {
        if let Some(in_flight) = self.by_transaction.get_mut(&transaction_hash) {
            in_flight.latest_proposal = in_flight.latest_proposal.max(proposal_timestamp);
            in_flight.finalized |= finalized;
            return;
        }
        for account in reservation.accounts() {
            let _ = self
                .by_account
                .entry(*account)
                .or_default()
                .insert(transaction_hash);
        }
        let _ = self.by_transaction.insert(
            transaction_hash,
            InFlight {
                reservation: reservation.clone(),
                latest_proposal: proposal_timestamp,
                finalized,
            },
        );
    }

    /// Records that a transaction holding a reservation is included in another proposal, or in a
    /// finalized block. Returns `false` if the transaction holds no reservation.
    pub(crate) fn extend(
        &mut self,
        transaction_hash: &TransactionHash,
        proposal_timestamp: Timestamp,
        finalized: bool,
    ) -> bool {
        match self.by_transaction.get_mut(transaction_hash) {
            Some(in_flight) => {
                in_flight.latest_proposal = in_flight.latest_proposal.max(proposal_timestamp);
                in_flight.finalized |= finalized;
                true
            }
            None => false,
        }
    }

    /// Returns whether `transaction_hash` holds a reservation.
    #[cfg(test)]
    pub(crate) fn contains(&self, transaction_hash: &TransactionHash) -> bool {
        self.by_transaction.contains_key(transaction_hash)
    }

    /// Releases the reservation of a transaction whose block has executed.
    pub(crate) fn release(&mut self, transaction_hash: &TransactionHash) {
        let Some(in_flight) = self.by_transaction.remove(transaction_hash) else {
            return;
        };
        for account in in_flight.reservation.accounts() {
            if let Some(holders) = self.by_account.get_mut(account) {
                let _ = holders.remove(transaction_hash);
                if holders.is_empty() {
                    let _ = self.by_account.remove(account);
                }
            }
        }
    }

    /// Releases the reservations of transactions in proposals no later than `finalized_timestamp`
    /// which were not finalized.
    pub(crate) fn release_abandoned(&mut self, finalized_timestamp: Timestamp) {
        let abandoned: Vec<_> = self
            .by_transaction
            .iter()
            .filter(|(_, in_flight)| {
                !in_flight.finalized && in_flight.latest_proposal <= finalized_timestamp
            })
            .map(|(transaction_hash, _)| *transaction_hash)
            .collect();
        for transaction_hash in abandoned {
            self.release(&transaction_hash);
        }
    }

    /// Returns an account of `reservation` reserved by a transaction other than
    /// `transaction_hash`, with that transaction.
    pub(crate) fn conflict(
        &self,
        transaction_hash: &TransactionHash,
        reservation: &EvmNonceReservation,
    ) -> Option<(evm::Address, TransactionHash)> {
        reservation.accounts().find_map(|account| {
            self.by_account
                .get(account)?
                .iter()
                .find(|holder| *holder != transaction_hash)
                .map(|holder| (*account, *holder))
        })
    }

    /// The number of transactions holding reservations.
    pub(crate) fn len(&self) -> usize {
        self.by_transaction.len()
    }

    /// The reservation held by `transaction_hash`, if any.
    #[cfg(test)]
    pub(crate) fn get(&self, transaction_hash: &TransactionHash) -> Option<&EvmNonceReservation> {
        self.by_transaction
            .get(transaction_hash)
            .map(|in_flight| &in_flight.reservation)
    }
}

#[cfg(test)]
mod tests {
    use casper_types::testing::TestRng;

    use super::*;

    fn address(byte: u8) -> evm::Address {
        evm::Address::new([byte; 20])
    }

    #[test]
    fn conflicts_with_any_reserved_account_of_another_transaction() {
        let rng = &mut TestRng::new();
        let (s1, s2, s3) = (address(1), address(2), address(3));
        let (t1, t2) = (TransactionHash::random(rng), TransactionHash::random(rng));
        let mut reservations = EvmReservations::default();
        let r1 = EvmNonceReservation::from_spans(s1, [(s1, 1, 4), (s2, 1, 1)]);
        reservations.reserve(t1, &r1, Timestamp::from(10), false);

        // The same transaction doesn't conflict with itself.
        assert_eq!(reservations.conflict(&t1, &r1), None);
        // Any nonce of a reserved account conflicts, even one past the reserved span.
        let r2 = EvmNonceReservation::from_spans(s3, [(s3, 0, 0), (s2, 9, 9)]);
        assert_eq!(reservations.conflict(&t2, &r2), Some((s2, t1)));
        let r3 = EvmNonceReservation::from_spans(s3, [(s3, 0, 0)]);
        assert_eq!(reservations.conflict(&t2, &r3), None);

        reservations.release(&t1);
        assert_eq!(reservations.conflict(&t2, &r2), None);
        assert_eq!(reservations.len(), 0);
    }

    #[test]
    fn abandoned_reservations_lapse_but_finalized_ones_are_held() {
        let rng = &mut TestRng::new();
        let s1 = address(1);
        let (t1, t2, t3) = (
            TransactionHash::random(rng),
            TransactionHash::random(rng),
            TransactionHash::random(rng),
        );
        let mut reservations = EvmReservations::default();
        let reservation = EvmNonceReservation::from_spans(s1, [(s1, 1, 1)]);
        reservations.reserve(t1, &reservation, Timestamp::from(10), false);
        reservations.reserve(t2, &reservation, Timestamp::from(10), false);
        reservations.reserve(t3, &reservation, Timestamp::from(30), false);
        assert!(reservations.extend(&t2, Timestamp::from(10), true));

        reservations.release_abandoned(Timestamp::from(20));
        assert!(!reservations.contains(&t1));
        assert!(reservations.contains(&t2));
        assert!(reservations.contains(&t3));

        // A later proposal including the transaction extends its reservation.
        assert!(reservations.extend(&t3, Timestamp::from(50), false));
        reservations.release_abandoned(Timestamp::from(40));
        assert!(reservations.contains(&t3));
    }
}
