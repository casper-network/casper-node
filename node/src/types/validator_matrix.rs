#[cfg(test)]
use std::iter;
use std::{
    collections::{BTreeMap, HashSet},
    fmt::{self, Debug, Formatter},
    sync::{Arc, RwLock, RwLockReadGuard},
};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use serde::Serialize;
use static_assertions::const_assert;
use tracing::info;

use casper_types::{EraId, PublicKey, SecretKey, U512};

use super::{BlockHeader, FinalitySignature};

const MAX_VALIDATOR_MATRIX_ENTRIES: usize = 6;
const_assert!(MAX_VALIDATOR_MATRIX_ENTRIES % 2 == 0);

#[derive(Eq, PartialEq, Debug, Copy, Clone, DataSize)]
pub(crate) enum SignatureWeight {
    /// Too few signatures to make any guarantees about the block's finality.
    Insufficient,
    /// At least one honest validator has signed the block.
    Weak,
    /// There can be no blocks on other forks that also have this many signatures.
    Strict,
}

impl SignatureWeight {
    pub(crate) fn is_sufficient(&self, requires_strict_finality: bool) -> bool {
        match self {
            SignatureWeight::Insufficient => false,
            SignatureWeight::Weak => false == requires_strict_finality,
            SignatureWeight::Strict => true,
        }
    }
}

#[derive(Clone, DataSize)]
pub(crate) struct ValidatorMatrix {
    inner: Arc<RwLock<BTreeMap<EraId, EraValidatorWeights>>>,
    chainspec_validators: Option<Arc<BTreeMap<PublicKey, U512>>>,
    chainspec_activation_era: EraId,
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
    secret_signing_key: Arc<SecretKey>,
    public_signing_key: PublicKey,
    auction_delay: u64,
    retrograde_latch: Option<EraId>,
}

impl ValidatorMatrix {
    pub(crate) fn new(
        finality_threshold_fraction: Ratio<u64>,
        chainspec_validators: Option<BTreeMap<PublicKey, U512>>,
        chainspec_activation_era: EraId,
        secret_signing_key: Arc<SecretKey>,
        public_signing_key: PublicKey,
        auction_delay: u64,
    ) -> Self {
        let inner = Arc::new(RwLock::new(BTreeMap::new()));
        ValidatorMatrix {
            inner,
            finality_threshold_fraction,
            chainspec_validators: chainspec_validators.map(Arc::new),
            chainspec_activation_era,
            secret_signing_key,
            public_signing_key,
            auction_delay,
            retrograde_latch: None,
        }
    }

    /// Creates a new validator matrix with just a single validator.
    #[cfg(test)]
    pub(crate) fn new_with_validator(secret_signing_key: Arc<SecretKey>) -> Self {
        let public_signing_key = PublicKey::from(&*secret_signing_key);
        let finality_threshold_fraction = Ratio::new(1, 3);
        let era_id = EraId::new(0);
        let weights = EraValidatorWeights::new(
            era_id,
            iter::once((public_signing_key.clone(), 100.into())).collect(),
            finality_threshold_fraction,
        );
        ValidatorMatrix {
            inner: Arc::new(RwLock::new(iter::once((era_id, weights)).collect())),
            chainspec_validators: None,
            chainspec_activation_era: EraId::from(0),
            finality_threshold_fraction,
            public_signing_key,
            secret_signing_key,
            auction_delay: 1,
            retrograde_latch: None,
        }
    }

    // Register the era of the highest orphaned block.
    pub(crate) fn register_retrograde_latch(&mut self, latch_era: Option<EraId>) {
        self.retrograde_latch = latch_era;
    }

    // When the chain starts, the validator weights will be the same until the unbonding delay is
    // elapsed. This allows us to possibly infer the weights of other eras if the era registered is
    // within the unbonding delay.
    // Currently we only infer the validator weights for era 0 from the set registered for era 1.
    // This is needed for the case where we want to sync leap to a block in era 0 of a pre 1.5.0
    // network for which we cant get the validator weights from a switch block.
    pub(crate) fn register_era_validator_weights(
        &mut self,
        validators: EraValidatorWeights,
    ) -> bool {
        let was_present = self.register_era_validator_weights_bounded(validators.clone());
        if validators.era_id() == EraId::from(1) {
            self.register_era_validator_weights_bounded(EraValidatorWeights::new(
                EraId::from(0),
                validators.validator_weights,
                validators.finality_threshold_fraction,
            ));
            info!("ValidatorMatrix: Inferred validator weights for Era 0 from weights in Era 1");
        }
        was_present
    }

    fn register_era_validator_weights_bounded(&mut self, validators: EraValidatorWeights) -> bool {
        let era_id = validators.era_id;
        let mut guard = self
            .inner
            .write()
            .expect("poisoned lock on validator matrix");
        let is_new = guard.insert(era_id, validators).is_none();

        let latch_era = if let Some(era) = self.retrograde_latch.as_ref() {
            *era
        } else {
            return is_new;
        };

        let mut removed = false;
        let excess_entry_count = guard.len().saturating_sub(MAX_VALIDATOR_MATRIX_ENTRIES);
        for _ in 0..excess_entry_count {
            let median_era = guard
                .keys()
                .rev()
                .nth(MAX_VALIDATOR_MATRIX_ENTRIES / 2)
                .copied()
                .unwrap();
            if median_era <= latch_era {
                break;
            } else {
                guard.remove(&median_era);
                if median_era == era_id {
                    removed = true;
                }
            }
        }
        is_new && !removed
    }

    pub(crate) fn register_validator_weights(
        &mut self,
        era_id: EraId,
        validator_weights: BTreeMap<PublicKey, U512>,
    ) {
        if self.read_inner().contains_key(&era_id) == false {
            self.register_era_validator_weights(EraValidatorWeights::new(
                era_id,
                validator_weights,
                self.finality_threshold_fraction,
            ));
        }
    }

    pub(crate) fn register_eras(
        &mut self,
        era_weights: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    ) {
        for (era_id, weights) in era_weights {
            self.register_validator_weights(era_id, weights);
        }
    }

    pub(crate) fn has_era(&self, era_id: &EraId) -> bool {
        self.read_inner().contains_key(era_id)
    }

    pub(crate) fn validator_weights(&self, era_id: EraId) -> Option<EraValidatorWeights> {
        if let (true, Some(chainspec_validators)) = (
            era_id == self.chainspec_activation_era,
            self.chainspec_validators.as_ref(),
        ) {
            Some(EraValidatorWeights::new(
                era_id,
                (**chainspec_validators).clone(),
                self.finality_threshold_fraction,
            ))
        } else {
            self.read_inner().get(&era_id).cloned()
        }
    }

    pub(crate) fn fault_tolerance_threshold(&self) -> Ratio<u64> {
        self.finality_threshold_fraction
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.read_inner().is_empty()
    }

    /// Returns whether `pub_key` is the ID of a validator in this era, or `None` if the validator
    /// information for that era is missing.
    pub(crate) fn is_validator_in_era(
        &self,
        era_id: EraId,
        public_key: &PublicKey,
    ) -> Option<bool> {
        if let (true, Some(chainspec_validators)) = (
            era_id == self.chainspec_activation_era,
            self.chainspec_validators.as_ref(),
        ) {
            Some(chainspec_validators.contains_key(public_key))
        } else {
            self.read_inner()
                .get(&era_id)
                .map(|validator_weights| validator_weights.is_validator(public_key))
        }
    }

    pub(crate) fn public_signing_key(&self) -> &PublicKey {
        &self.public_signing_key
    }

    pub(crate) fn secret_signing_key(&self) -> &Arc<SecretKey> {
        &self.secret_signing_key
    }

    /// Returns whether `pub_key` is the ID of a validator in this era, or `None` if the validator
    /// information for that era is missing.
    pub(crate) fn is_self_validator_in_era(&self, era_id: EraId) -> Option<bool> {
        self.is_validator_in_era(era_id, &self.public_signing_key)
    }

    /// Determine if the active validator is in a current or upcoming set of active validators.
    #[inline]
    pub(crate) fn is_active_or_upcoming_validator(&self, public_key: &PublicKey) -> bool {
        // This function is potentially expensive and could be memoized, with the cache being
        // invalidated when the max value of the `BTreeMap` changes.
        self.read_inner()
            .values()
            .rev()
            .take(self.auction_delay as usize + 1)
            .any(|validator_weights| validator_weights.is_validator(public_key))
    }

    pub(crate) fn create_finality_signature(
        &self,
        block_header: &BlockHeader,
    ) -> Option<FinalitySignature> {
        if self
            .is_self_validator_in_era(block_header.era_id())
            .unwrap_or(false)
        {
            return Some(FinalitySignature::create(
                block_header.block_hash(),
                block_header.era_id(),
                &self.secret_signing_key,
                self.public_signing_key.clone(),
            ));
        }
        None
    }

    fn read_inner(&self) -> RwLockReadGuard<BTreeMap<EraId, EraValidatorWeights>> {
        self.inner.read().unwrap()
    }

    pub(crate) fn eras(&self) -> Vec<EraId> {
        self.read_inner().keys().copied().collect_vec()
    }
}

impl Debug for ValidatorMatrix {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatorMatrix")
            .field("weights", &*self.read_inner())
            .field(
                "finality_threshold_fraction",
                &self.finality_threshold_fraction,
            )
            .finish()
    }
}

#[derive(DataSize, Debug, Eq, PartialEq, Serialize, Default, Clone)]
pub(crate) struct EraValidatorWeights {
    era_id: EraId,
    validator_weights: BTreeMap<PublicKey, U512>,
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
}

impl EraValidatorWeights {
    pub(crate) fn new(
        era_id: EraId,
        validator_weights: BTreeMap<PublicKey, U512>,
        finality_threshold_fraction: Ratio<u64>,
    ) -> Self {
        EraValidatorWeights {
            era_id,
            validator_weights,
            finality_threshold_fraction,
        }
    }

    pub(crate) fn era_id(&self) -> EraId {
        self.era_id
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.validator_weights.is_empty()
    }

    pub(crate) fn get_total_weight(&self) -> U512 {
        self.validator_weights.values().copied().sum()
    }

    pub(crate) fn validator_public_keys(&self) -> impl Iterator<Item = &PublicKey> {
        self.validator_weights.keys()
    }

    pub(crate) fn into_validator_public_keys(self) -> impl Iterator<Item = PublicKey> {
        self.validator_weights.into_keys()
    }

    pub(crate) fn missing_validators<'a>(
        &self,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> impl Iterator<Item = &PublicKey> {
        let provided_keys: HashSet<_> = validator_keys.cloned().collect();
        self.validator_weights
            .keys()
            .filter(move |&validator| !provided_keys.contains(validator))
    }

    pub(crate) fn bogus_validators<'a>(
        &self,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> Vec<PublicKey> {
        validator_keys
            .filter(move |validator_key| !self.validator_weights.keys().contains(validator_key))
            .cloned()
            .collect()
    }

    pub(crate) fn get_weight(&self, public_key: &PublicKey) -> U512 {
        match self.validator_weights.get(public_key) {
            None => U512::zero(),
            Some(w) => *w,
        }
    }

    pub(crate) fn is_validator(&self, public_key: &PublicKey) -> bool {
        self.validator_weights.contains_key(public_key)
    }

    pub(crate) fn signed_weight<'a>(
        &self,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> U512 {
        validator_keys
            .map(|validator_key| self.get_weight(validator_key))
            .sum()
    }

    pub(crate) fn signature_weight<'a>(
        &self,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> SignatureWeight {
        // sufficient is ~33.4%, strict is ~66.7% by default in highway
        // in some cases, we may already have strict weight or better before even starting.
        // this is optimal, but in the cases where we do not we are willing to start work
        // on acquiring block data on a block for which we have at least sufficient weight.
        // nevertheless, we will try to attain strict weight before fully accepting such
        // a block.
        let finality_threshold_fraction = self.finality_threshold_fraction;
        let strict = Ratio::new(1, 2) * (Ratio::from_integer(1) + finality_threshold_fraction);
        let total_era_weight = self.get_total_weight();

        let signature_weight = self.signed_weight(validator_keys);
        if signature_weight * U512::from(*strict.denom())
            > total_era_weight * U512::from(*strict.numer())
        {
            return SignatureWeight::Strict;
        }
        if signature_weight * U512::from(*finality_threshold_fraction.denom())
            > total_era_weight * U512::from(*finality_threshold_fraction.numer())
        {
            return SignatureWeight::Weak;
        }
        SignatureWeight::Insufficient
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use casper_types::EraId;
    use num_rational::Ratio;

    use crate::{
        components::consensus::tests::utils::{
            ALICE_PUBLIC_KEY, ALICE_SECRET_KEY, BOB_PUBLIC_KEY, CAROL_PUBLIC_KEY,
        },
        types::{validator_matrix::MAX_VALIDATOR_MATRIX_ENTRIES, SignatureWeight},
    };

    use super::{EraValidatorWeights, ValidatorMatrix};

    fn empty_era_validator_weights(era_id: EraId) -> EraValidatorWeights {
        EraValidatorWeights::new(
            era_id,
            iter::once((ALICE_PUBLIC_KEY.clone(), 100.into())).collect(),
            Ratio::new(1, 3),
        )
    }

    #[test]
    fn signature_weight_at_boundary_equal_weights() {
        let weights = EraValidatorWeights::new(
            EraId::default(),
            [
                (ALICE_PUBLIC_KEY.clone(), 100.into()),
                (BOB_PUBLIC_KEY.clone(), 100.into()),
                (CAROL_PUBLIC_KEY.clone(), 100.into()),
            ]
            .into(),
            Ratio::new(1, 3),
        );

        assert_eq!(
            weights.signature_weight([ALICE_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Insufficient
        );
        assert_eq!(
            weights.signature_weight([BOB_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Insufficient
        );
        assert_eq!(
            weights.signature_weight([CAROL_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Insufficient
        );
        assert_eq!(
            weights.signature_weight([ALICE_PUBLIC_KEY.clone(), BOB_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Weak
        );
        assert_eq!(
            weights.signature_weight([ALICE_PUBLIC_KEY.clone(), CAROL_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Weak
        );
        assert_eq!(
            weights.signature_weight([BOB_PUBLIC_KEY.clone(), CAROL_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Weak
        );
        assert_eq!(
            weights.signature_weight(
                [
                    ALICE_PUBLIC_KEY.clone(),
                    BOB_PUBLIC_KEY.clone(),
                    CAROL_PUBLIC_KEY.clone()
                ]
                .iter()
            ),
            SignatureWeight::Strict
        );
    }

    #[test]
    fn signature_weight_at_boundary_unequal_weights() {
        let weights = EraValidatorWeights::new(
            EraId::default(),
            [
                (ALICE_PUBLIC_KEY.clone(), 101.into()),
                (BOB_PUBLIC_KEY.clone(), 100.into()),
                (CAROL_PUBLIC_KEY.clone(), 100.into()),
            ]
            .into(),
            Ratio::new(1, 3),
        );

        assert_eq!(
            weights.signature_weight([ALICE_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Weak
        );
        assert_eq!(
            weights.signature_weight([BOB_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Insufficient
        );
        assert_eq!(
            weights.signature_weight([CAROL_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Insufficient
        );
        assert_eq!(
            weights.signature_weight([ALICE_PUBLIC_KEY.clone(), BOB_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Strict
        );
        assert_eq!(
            weights.signature_weight([ALICE_PUBLIC_KEY.clone(), CAROL_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Strict
        );
        assert_eq!(
            weights.signature_weight([BOB_PUBLIC_KEY.clone(), CAROL_PUBLIC_KEY.clone()].iter()),
            SignatureWeight::Weak
        );
        assert_eq!(
            weights.signature_weight(
                [
                    ALICE_PUBLIC_KEY.clone(),
                    BOB_PUBLIC_KEY.clone(),
                    CAROL_PUBLIC_KEY.clone()
                ]
                .iter()
            ),
            SignatureWeight::Strict
        );
    }

    #[test]
    fn register_validator_weights_pruning() {
        // Create a validator matrix and saturate it with entries.
        let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
        let mut era_validator_weights = vec![validator_matrix.validator_weights(0.into()).unwrap()];
        era_validator_weights.extend(
            (1..MAX_VALIDATOR_MATRIX_ENTRIES as u64)
                .into_iter()
                .map(EraId::from)
                .map(empty_era_validator_weights),
        );
        for evw in era_validator_weights
            .iter()
            .take(MAX_VALIDATOR_MATRIX_ENTRIES)
            .skip(1)
            .cloned()
        {
            assert!(
                validator_matrix.register_era_validator_weights(evw),
                "register_era_validator_weights"
            );
        }
        // For a `MAX_VALIDATOR_MATRIX_ENTRIES` value of 6, the validator
        // matrix should contain eras 0 through 5 inclusive.
        assert_eq!(
            vec![0u64, 1, 2, 3, 4, 5],
            validator_matrix
                .read_inner()
                .keys()
                .copied()
                .map(EraId::value)
                .collect::<Vec<u64>>()
        );

        // Now that we have 6 entries in the validator matrix, try adding more.
        // We should have an entry for era 3 (we have eras 0 through 5
        // inclusive).
        let median = MAX_VALIDATOR_MATRIX_ENTRIES as u64 / 2;
        assert!(
            validator_matrix.has_era(&(median).into()),
            "should have median era {}",
            median
        );
        // Add era 7, which would be the 7th entry in the matrix. Skipping era
        // 6 should have no effect on the pruning.
        era_validator_weights.push(empty_era_validator_weights(
            (MAX_VALIDATOR_MATRIX_ENTRIES as u64 + 1).into(),
        ));

        // set retrograde latch to simulate a fully sync'd node
        validator_matrix.register_retrograde_latch(Some(EraId::new(0)));

        // Now the entry for era 3 should be dropped and we should be left with
        // the 3 lowest eras [0, 1, 2] and 3 highest eras [4, 5, 7].
        assert!(validator_matrix
            .register_era_validator_weights(era_validator_weights.last().cloned().unwrap()));
        assert!(
            !validator_matrix.has_era(&(median).into()),
            "should not have median era {}",
            median
        );
        let len = validator_matrix.read_inner().len();
        assert_eq!(
            len, MAX_VALIDATOR_MATRIX_ENTRIES,
            "expected entries {} actual entries: {}",
            MAX_VALIDATOR_MATRIX_ENTRIES, len
        );
        let expected = vec![0u64, 1, 2, 4, 5, 7];
        let actual = validator_matrix
            .read_inner()
            .keys()
            .copied()
            .map(EraId::value)
            .collect::<Vec<u64>>();
        assert_eq!(expected, actual, "{:?} {:?}", expected, actual);

        // Adding existing eras shouldn't change the state.
        let old_state: Vec<EraId> = validator_matrix.read_inner().keys().copied().collect();
        let repeat = era_validator_weights
            .last()
            .cloned()
            .expect("should have last entry");
        assert!(
            !validator_matrix.register_era_validator_weights(repeat),
            "should not re-register already registered era"
        );
        let new_state: Vec<EraId> = validator_matrix.read_inner().keys().copied().collect();
        assert_eq!(old_state, new_state, "state should be unchanged");
    }

    #[test]
    fn register_validator_weights_latched_pruning() {
        // Create a validator matrix and saturate it with entries.
        let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
        // Set the retrograde latch to 10 so we can register all eras lower or
        // equal to 10.
        validator_matrix.register_retrograde_latch(Some(EraId::from(10)));
        let mut era_validator_weights = vec![validator_matrix.validator_weights(0.into()).unwrap()];
        era_validator_weights.extend(
            (1..=MAX_VALIDATOR_MATRIX_ENTRIES as u64)
                .into_iter()
                .map(EraId::from)
                .map(empty_era_validator_weights),
        );
        for evw in era_validator_weights
            .iter()
            .take(MAX_VALIDATOR_MATRIX_ENTRIES + 1)
            .skip(1)
            .cloned()
        {
            assert!(
                validator_matrix.register_era_validator_weights(evw),
                "register_era_validator_weights"
            );
        }

        // Register eras [7, 8, 9].
        era_validator_weights.extend(
            (7..=9)
                .into_iter()
                .map(EraId::from)
                .map(empty_era_validator_weights),
        );
        for evw in era_validator_weights.iter().rev().take(3).cloned() {
            assert!(
                validator_matrix.register_era_validator_weights(evw),
                "register_era_validator_weights"
            );
        }

        // Set the retrograde latch to era 5.
        validator_matrix.register_retrograde_latch(Some(EraId::from(5)));
        // Add era 10 to the weights.
        era_validator_weights.push(empty_era_validator_weights(EraId::from(10)));
        assert_eq!(era_validator_weights.len(), 11);
        // As the current weights in the matrix are [0, ..., 9], register era
        // 10. This should succeed anyway since it's the highest weight.
        assert!(
            validator_matrix.register_era_validator_weights(era_validator_weights[10].clone()),
            "register_era_validator_weights"
        );
        // The latch was previously set to 5, so now all weights which are
        // neither the lowest 3, highest 3 or higher than the latched era
        // should have been purged.
        // Given we had weights [0, ..., 10] and the latch is 5, we should
        // be left with [0, 1, 2, 3, 4, 5, 8, 9, 10].
        for era in 0..=5 {
            assert!(validator_matrix.has_era(&EraId::from(era)));
        }
        for era in 6..=7 {
            assert!(!validator_matrix.has_era(&EraId::from(era)));
        }
        for era in 8..=10 {
            assert!(validator_matrix.has_era(&EraId::from(era)));
        }

        // Make sure era 6, which was previously purged, is not registered as
        // it is greater than the latch, which is 5.
        assert!(
            !validator_matrix.register_era_validator_weights(era_validator_weights[6].clone()),
            "register_era_validator_weights"
        );

        // Set the retrograde latch to era 6.
        validator_matrix.register_retrograde_latch(Some(EraId::from(6)));
        // Make sure era 6 is now registered.
        assert!(
            validator_matrix.register_era_validator_weights(era_validator_weights[6].clone()),
            "register_era_validator_weights"
        );

        // Set the retrograde latch to era 1.
        validator_matrix.register_retrograde_latch(Some(EraId::from(1)));
        // Register era 10 again to drive the purging mechanism.
        assert!(
            !validator_matrix.register_era_validator_weights(era_validator_weights[10].clone()),
            "register_era_validator_weights"
        );
        // The latch was previously set to 1, so now all weights which are
        // neither the lowest 3, highest 3 or higher than the latched era
        // should have been purged.
        // Given we had weights [0, 1, 2, 3, 4, 5, 6, 8, 9, 10] and the latch
        // is 1, we should be left with [0, 1, 2, 8, 9, 10].
        for era in 0..=2 {
            assert!(validator_matrix.has_era(&EraId::from(era)));
        }
        for era in 3..=7 {
            assert!(!validator_matrix.has_era(&EraId::from(era)));
        }
        for era in 8..=10 {
            assert!(validator_matrix.has_era(&EraId::from(era)));
        }
    }
}
