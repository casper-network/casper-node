use std::{
    collections::{hash_map::Entry, BTreeSet, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    iter, mem,
};

use datasize::DataSize;
use tracing::{debug, error, warn};

use casper_types::{
    Approval, ApprovalsHash, Chainspec, FinalitySignatureId, Timestamp, TransactionConfig,
    TransactionHash,
};

use crate::{
    components::consensus::{ClContext, ProposedBlock},
    effect::Responder,
    types::{appendable_block::AppendableBlock, NodeId, TransactionFootprint},
};

/// The state of a peer which claims to be a holder of the deploys.
#[derive(Clone, Copy, Eq, PartialEq, DataSize, Debug)]
pub(super) enum HolderState {
    /// No fetch attempt has been made using this peer.
    Unasked,
    /// At least one fetch attempt has been made and no fetch attempts have failed when using this
    /// peer.
    Asked,
    /// At least one fetch attempt has failed when using this peer.
    Failed,
}

/// The return type of `BlockValidationState::add_responder`.
pub(super) enum AddResponderResult {
    /// The responder was added, meaning validation is still ongoing.
    Added,
    /// Validation is completed, so the responder should be called with the provided value.
    ValidationCompleted {
        responder: Responder<bool>,
        response_to_send: bool,
    },
}

/// The return type of `BlockValidationState::start_fetching`.
#[derive(Eq, PartialEq, Debug)]
pub(super) enum MaybeStartFetching {
    /// Should start a new round of fetches.
    Start {
        holder: NodeId,
        missing_transactions: HashMap<TransactionHash, ApprovalsHash>,
        missing_signatures: HashSet<FinalitySignatureId>,
    },
    /// No new round of fetches should be started as one is already in progress.
    Ongoing,
    /// We still have missing transactions, but all holders have failed.
    Unable,
    /// Validation has succeeded already.
    ValidationSucceeded,
    /// Validation has failed already.
    ValidationFailed,
}

#[derive(Clone, Eq, PartialEq, DataSize, Debug)]
pub(super) struct ApprovalInfo {
    approvals: BTreeSet<Approval>,
    approvals_hash: ApprovalsHash,
}

impl ApprovalInfo {
    fn new(approvals: BTreeSet<Approval>, approvals_hash: ApprovalsHash) -> Self {
        ApprovalInfo {
            approvals,
            approvals_hash,
        }
    }
}

/// State of the current process of block validation.
///
/// Tracks whether or not there are transactions still missing and who is interested in the final
/// result.
#[derive(DataSize, Debug)]
pub(super) enum BlockValidationState {
    /// The validity is not yet decided.
    InProgress {
        /// Appendable block ensuring that the transactions satisfy the validity conditions.
        appendable_block: AppendableBlock,
        /// The set of approvals contains approvals from transactions that would be finalized with
        /// the block.
        missing_transactions: HashMap<TransactionHash, ApprovalInfo>,
        /// The set of finality signatures for past blocks cited in this block.
        missing_signatures: HashSet<FinalitySignatureId>,
        /// The set of peers which each claim to hold all the transactions.
        holders: HashMap<NodeId, HolderState>,
        /// A list of responders that are awaiting an answer.
        responders: Vec<Responder<bool>>,
    },
    /// The proposed block with the given timestamp is valid.
    Valid(Timestamp),
    /// The proposed block with the given timestamp is invalid.
    ///
    /// Note that only hard failures in validation will result in this state.  For soft failures,
    /// like failing to fetch from a peer, the state will remain `Unknown`, even if there are no
    /// more peers to ask, since more peers could be provided before this `BlockValidationState` is
    /// purged.
    Invalid(Timestamp),
}

impl BlockValidationState {
    /// Returns a new `BlockValidationState`.
    ///
    /// If the new state is `Valid` or `Invalid`, the provided responder is also returned so it can
    /// be actioned.
    pub(super) fn new(
        proposed_block: &ProposedBlock<ClContext>,
        missing_signatures: HashSet<FinalitySignatureId>,
        sender: NodeId,
        responder: Responder<bool>,
        chainspec: &Chainspec,
    ) -> (Self, Option<Responder<bool>>) {
        let transaction_count = proposed_block.transaction_count();
        if transaction_count == 0 {
            let state = BlockValidationState::Valid(proposed_block.timestamp());
            return (state, Some(responder));
        }

        if Self::validate_transaction_category_counts(proposed_block, &chainspec.transaction_config)
            .is_err()
        {
            let state = BlockValidationState::Invalid(proposed_block.timestamp());
            return (state, Some(responder));
        }

        let appendable_block =
            AppendableBlock::new(chainspec.transaction_config, proposed_block.timestamp());

        let mut missing_transactions = HashMap::new();

        for (transaction_hash, approvals) in proposed_block.all_transactions() {
            let approval_info: ApprovalInfo = match transaction_hash {
                TransactionHash::Deploy(_) => match ApprovalsHash::compute(approvals) {
                    Ok(approvals_hash) => ApprovalInfo::new(approvals.clone(), approvals_hash),
                    Err(error) => {
                        warn!(%transaction_hash, %error, "could not compute approvals hash");
                        let state = BlockValidationState::Invalid(proposed_block.timestamp());
                        return (state, Some(responder));
                    }
                },
                TransactionHash::V1(_) => match ApprovalsHash::compute(approvals) {
                    Ok(approvals_hash) => ApprovalInfo::new(approvals.clone(), approvals_hash),
                    Err(error) => {
                        warn!(%transaction_hash, %error, "could not compute approvals hash");
                        let state = BlockValidationState::Invalid(proposed_block.timestamp());
                        return (state, Some(responder));
                    }
                },
            };

            if missing_transactions
                .insert(*transaction_hash, approval_info)
                .is_some()
            {
                warn!(%transaction_hash, "duplicated transaction in proposed block");
                let state = BlockValidationState::Invalid(proposed_block.timestamp());
                return (state, Some(responder));
            }
        }

        let state = BlockValidationState::InProgress {
            appendable_block,
            missing_transactions,
            missing_signatures,
            holders: iter::once((sender, HolderState::Unasked)).collect(),
            responders: vec![responder],
        };

        (state, None)
    }

    fn validate_transaction_category_counts(
        block: &ProposedBlock<ClContext>,
        config: &TransactionConfig,
    ) -> Result<(), ()> {
        if block.standard_count() > config.block_max_standard_count as usize {
            warn!("too many standard transactions");
            return Err(());
        }
        if block.auction_count() > config.block_max_auction_count as usize {
            warn!("too many staking transactions");
            return Err(());
        }
        if block.install_upgrade_count() > config.block_max_install_upgrade_count as usize {
            warn!("too many install_upgrade transactions");
            return Err(());
        }
        if block.mint_count() > config.block_max_mint_count as usize {
            warn!("too many transfers");
            return Err(());
        }

        Ok(())
    }

    /// Adds the given responder to the collection if the current state is `InProgress` and returns
    /// `Added`.
    ///
    /// If the state is not `InProgress`, `ValidationCompleted` is returned with the responder and
    /// the value which should be provided to the responder.
    pub(super) fn add_responder(&mut self, responder: Responder<bool>) -> AddResponderResult {
        match self {
            BlockValidationState::InProgress { responders, .. } => {
                responders.push(responder);
                AddResponderResult::Added
            }
            BlockValidationState::Valid(_) => AddResponderResult::ValidationCompleted {
                responder,
                response_to_send: true,
            },
            BlockValidationState::Invalid(_) => AddResponderResult::ValidationCompleted {
                responder,
                response_to_send: false,
            },
        }
    }

    /// If the current state is `InProgress` and the peer isn't already known, adds the peer.
    /// Otherwise any existing entry is not updated and `false` is returned.
    pub(super) fn add_holder(&mut self, holder: NodeId) {
        match self {
            BlockValidationState::InProgress {
                appendable_block,
                holders,
                ..
            } => match holders.entry(holder) {
                Entry::Occupied(entry) => {
                    debug!(
                        block_timestamp = %appendable_block.timestamp(),
                        peer = %entry.key(),
                        "already registered peer as holder for block validation"
                    );
                }
                Entry::Vacant(entry) => {
                    entry.insert(HolderState::Unasked);
                }
            },
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => {
                error!(state = %self, "unexpected state when adding holder");
            }
        }
    }

    /// If the current state is `InProgress` and the holder is present, sets the holder's state to
    /// `Failed`.
    pub(super) fn try_mark_holder_failed(&mut self, holder: &NodeId) {
        if let BlockValidationState::InProgress { holders, .. } = self {
            if let Some(holder_state) = holders.get_mut(holder) {
                debug_assert!(*holder_state != HolderState::Unasked);
                *holder_state = HolderState::Failed;
            }
        }
    }

    /// Returns fetch info based on the current state:
    ///   * if `InProgress` and there are no holders `Asked` (i.e. no ongoing fetches) and at least
    ///     one `Unasked` holder, returns `Start`
    ///   * if `InProgress` and any holder `Asked`, returns `Ongoing`
    ///   * if `InProgress` and all holders `Failed`, returns `Unable`
    ///   * if `Valid` or `Invalid`, returns `ValidationSucceeded` or `ValidationFailed`
    ///     respectively
    pub(super) fn start_fetching(&mut self) -> MaybeStartFetching {
        match self {
            BlockValidationState::InProgress {
                missing_transactions,
                missing_signatures,
                holders,
                ..
            } => {
                if missing_transactions.is_empty() && missing_signatures.is_empty() {
                    error!(
                        "should always have missing transactions or signatures while in state \
                        `InProgress`"
                    );
                    debug_assert!(false, "invalid state");
                    return MaybeStartFetching::ValidationFailed;
                }
                let mut unasked = None;
                for (peer_id, holder_state) in holders.iter() {
                    match holder_state {
                        HolderState::Unasked => {
                            unasked = Some(*peer_id);
                        }
                        HolderState::Asked => return MaybeStartFetching::Ongoing,
                        HolderState::Failed => {}
                    }
                }

                let holder = match unasked {
                    Some(peer) => peer,
                    None => return MaybeStartFetching::Unable,
                };
                // Mark the holder as `Asked`.  Safe to `expect` as we just found the entry above.
                *holders.get_mut(&holder).expect("must be in set") = HolderState::Asked;
                let missing_transactions = missing_transactions
                    .iter()
                    .map(|(dt_hash, infos)| (*dt_hash, infos.approvals_hash))
                    .collect();
                let missing_signatures = missing_signatures.clone();
                MaybeStartFetching::Start {
                    holder,
                    missing_transactions,
                    missing_signatures,
                }
            }
            BlockValidationState::Valid(_) => MaybeStartFetching::ValidationSucceeded,
            BlockValidationState::Invalid(_) => MaybeStartFetching::ValidationFailed,
        }
    }

    pub(super) fn take_responders(&mut self) -> Vec<Responder<bool>> {
        match self {
            BlockValidationState::InProgress { responders, .. } => mem::take(responders),
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => vec![],
        }
    }

    /// If the current state is `InProgress` and `dt_hash` is present, tries to add the footprint to
    /// the appendable block to continue validation of the proposed block.
    pub(super) fn try_add_transaction_footprint(
        &mut self,
        transaction_hash: &TransactionHash,
        footprint: &TransactionFootprint,
    ) -> Vec<Responder<bool>> {
        let (new_state, responders) = match self {
            BlockValidationState::InProgress {
                appendable_block,
                missing_transactions,
                responders,
                ..
            } => {
                let approvals_info = match missing_transactions.remove(transaction_hash) {
                    Some(info) => info,
                    None => {
                        // If this deploy is not present, just return.
                        return vec![];
                    }
                };
                // Try adding the footprint to the appendable block to see if the block remains
                // valid.
                let approvals = approvals_info.approvals;
                let footprint = footprint.clone().with_approvals(approvals);
                match appendable_block.add_transaction(footprint.clone()) {
                    Ok(_) => {
                        if !missing_transactions.is_empty() {
                            // The appendable block is still valid, but we still have missing
                            // deploys - nothing further to do here.
                            debug!(
                                block_timestamp = %appendable_block.timestamp(),
                                missing_transactions_len = missing_transactions.len(),
                                "still missing transactions - block validation incomplete"
                            );
                            return vec![];
                        }
                        debug!(
                            block_timestamp = %appendable_block.timestamp(),
                            "no further missing transactions - block validation complete"
                        );
                        let new_state = BlockValidationState::Valid(appendable_block.timestamp());
                        (new_state, mem::take(responders))
                    }
                    Err(error) => {
                        warn!(%transaction_hash, ?footprint, %error, "block invalid");
                        let new_state = BlockValidationState::Invalid(appendable_block.timestamp());
                        (new_state, mem::take(responders))
                    }
                }
            }
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => return vec![],
        };
        *self = new_state;
        responders
    }

    /// If the current state is `InProgress` and `dt_hash` is present, tries to add the footprint to
    /// the appendable block to continue validation of the proposed block.
    pub(super) fn try_add_signature(
        &mut self,
        finality_signature_id: &FinalitySignatureId,
    ) -> Vec<Responder<bool>> {
        let (new_state, responders) = match self {
            BlockValidationState::InProgress {
                appendable_block,
                missing_transactions,
                missing_signatures,
                responders,
                ..
            } => {
                missing_signatures.remove(finality_signature_id);
                if missing_signatures.is_empty() && missing_transactions.is_empty() {
                    debug!(
                        block_timestamp = %appendable_block.timestamp(),
                        "no further missing transactions or signatures - block validation complete"
                    );
                    let new_state = BlockValidationState::Valid(appendable_block.timestamp());
                    (new_state, mem::take(responders))
                } else {
                    debug!(
                        block_timestamp = %appendable_block.timestamp(),
                        missing_transactions_len = missing_transactions.len(),
                        missing_signatures_len = missing_signatures.len(),
                        "still missing transactions or signatures - block validation incomplete"
                    );
                    return vec![];
                }
            }
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => return vec![],
        };
        *self = new_state;
        responders
    }

    /// If the current state is `InProgress` and `dt_hash` is present, sets the state to `Invalid`
    /// and returns the responders.
    pub(super) fn try_mark_invalid(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Vec<Responder<bool>> {
        let (timestamp, responders) = match self {
            BlockValidationState::InProgress {
                appendable_block,
                missing_transactions,
                responders,
                ..
            } => {
                if !missing_transactions.contains_key(transaction_hash) {
                    return vec![];
                }
                (appendable_block.timestamp(), mem::take(responders))
            }
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => return vec![],
        };
        *self = BlockValidationState::Invalid(timestamp);
        responders
    }

    /// If the current state is `InProgress` and `finality_signature_id` is present, sets the state
    /// to `Invalid` and returns the responders.
    pub(super) fn try_mark_invalid_signature(
        &mut self,
        finality_signature_id: &FinalitySignatureId,
    ) -> Vec<Responder<bool>> {
        let (timestamp, responders) = match self {
            BlockValidationState::InProgress {
                appendable_block,
                missing_signatures,
                responders,
                ..
            } => {
                if !missing_signatures.contains(finality_signature_id) {
                    return vec![];
                }
                (appendable_block.timestamp(), mem::take(responders))
            }
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => return vec![],
        };
        *self = BlockValidationState::Invalid(timestamp);
        responders
    }

    pub(super) fn block_timestamp_if_completed(&self) -> Option<Timestamp> {
        match self {
            BlockValidationState::InProgress { .. } => None,
            BlockValidationState::Valid(timestamp) | BlockValidationState::Invalid(timestamp) => {
                Some(*timestamp)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn missing_hashes(&self) -> Vec<TransactionHash> {
        match self {
            BlockValidationState::InProgress {
                missing_transactions,
                ..
            } => missing_transactions
                .keys()
                .map(|dt_hash| match dt_hash {
                    TransactionHash::Deploy(deploy) => deploy.deploy_hash().into(),
                    TransactionHash::V1(v1) => (*v1).into(),
                })
                .collect(),
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => vec![],
        }
    }

    #[cfg(test)]
    pub(super) fn holders_mut(&mut self) -> Option<&mut HashMap<NodeId, HolderState>> {
        match self {
            BlockValidationState::InProgress { holders, .. } => Some(holders),
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => None,
        }
    }

    #[cfg(test)]
    pub(super) fn responder_count(&self) -> usize {
        match self {
            BlockValidationState::InProgress { responders, .. } => responders.len(),
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => 0,
        }
    }

    #[cfg(test)]
    pub(super) fn completed(&self) -> bool {
        !matches!(self, BlockValidationState::InProgress { .. })
    }
}

impl Display for BlockValidationState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockValidationState::InProgress {
                appendable_block,
                missing_transactions,
                missing_signatures,
                holders,
                responders,
            } => {
                write!(
                    formatter,
                    "BlockValidationState::InProgress({}, {} missing transactions, \
                    {} missing signatures, {} holders, {} responders)",
                    appendable_block,
                    missing_transactions.len(),
                    missing_signatures.len(),
                    holders.len(),
                    responders.len()
                )
            }
            BlockValidationState::Valid(timestamp) => {
                write!(formatter, "BlockValidationState::Valid({timestamp})")
            }
            BlockValidationState::Invalid(timestamp) => {
                write!(formatter, "BlockValidationState::Invalid({timestamp})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::channel::oneshot;
    use rand::Rng;
    use std::hash::Hash;
    use BTreeSet;

    use casper_types::{
        testing::TestRng, ChainspecRawBytes, TimeDiff, Transaction, TransactionHash,
    };

    use super::{super::tests::*, *};
    use crate::{types::TransactionExt, utils::Loadable};

    struct Fixture<'a> {
        rng: &'a mut TestRng,
        transactions: Vec<Transaction>,
        chainspec: Chainspec,
    }

    impl<'a> Fixture<'a> {
        fn new(rng: &'a mut TestRng) -> Self {
            let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
            Fixture {
                rng,
                transactions: vec![],
                chainspec,
            }
        }

        fn footprints(&self) -> Vec<(TransactionHash, TransactionFootprint)> {
            self.transactions
                .iter()
                .map(|transaction| (transaction.hash(), transaction.footprint(&self.chainspec)))
                .collect()
        }
    }

    fn new_responder() -> Responder<bool> {
        let (sender, _receiver) = oneshot::channel();
        Responder::without_shutdown(sender)
    }

    // Please note: values in the following test cases must much the production chainspec.
    const MAX_STANDARD_COUNT: u64 = 100;
    const MAX_STAKING_COUNT: u64 = 200;
    const MAX_INSTALL_UPGRADE_COUNT: u64 = 2;
    const MAX_TRANSFER_COUNT: u64 = 1000;

    struct TestCase {
        transfer_count: u64,
        staking_count: u64,
        install_upgrade_count: u64,
        standard_count: u64,
        state_validator: fn((BlockValidationState, Option<Responder<bool>>)) -> bool,
    }

    const NO_TRANSACTIONS: TestCase = TestCase {
        transfer_count: 0,
        staking_count: 0,
        install_upgrade_count: 0,
        standard_count: 0,
        state_validator: |(state, responder)| {
            responder.is_some() && matches!(state, BlockValidationState::Valid(_))
        },
    };

    const FULL_STAKING: TestCase = TestCase {
        transfer_count: 0,
        staking_count: MAX_STAKING_COUNT,
        install_upgrade_count: 0,
        standard_count: 0,
        state_validator: |(state, responder)| {
            responder.is_none() && matches!(state, BlockValidationState::InProgress { .. })
        },
    };

    const LESS_THAN_MAX_STAKING: TestCase = TestCase {
        staking_count: FULL_STAKING.staking_count - 1,
        state_validator: |(state, responder)| {
            responder.is_none() && matches!(state, BlockValidationState::InProgress { .. })
        },
        ..FULL_STAKING
    };

    const TOO_MANY_STAKING: TestCase = TestCase {
        staking_count: FULL_STAKING.staking_count + 1,
        state_validator: |(state, responder)| {
            responder.is_some() && matches!(state, BlockValidationState::Invalid(_))
        },
        ..FULL_STAKING
    };

    const FULL_INSTALL_UPGRADE: TestCase = TestCase {
        transfer_count: 0,
        staking_count: 0,
        install_upgrade_count: MAX_INSTALL_UPGRADE_COUNT,
        standard_count: 0,
        state_validator: |(state, responder)| {
            responder.is_none() && matches!(state, BlockValidationState::InProgress { .. })
        },
    };

    const LESS_THAN_MAX_INSTALL_UPGRADE: TestCase = TestCase {
        install_upgrade_count: FULL_INSTALL_UPGRADE.install_upgrade_count - 1,
        state_validator: |(state, responder)| {
            responder.is_none() && matches!(state, BlockValidationState::InProgress { .. })
        },
        ..FULL_INSTALL_UPGRADE
    };

    const TOO_MANY_INSTALL_UPGRADE: TestCase = TestCase {
        install_upgrade_count: FULL_INSTALL_UPGRADE.install_upgrade_count + 1,
        state_validator: |(state, responder)| {
            responder.is_some() && matches!(state, BlockValidationState::Invalid(_))
        },
        ..FULL_INSTALL_UPGRADE
    };

    const FULL_STANDARD: TestCase = TestCase {
        transfer_count: 0,
        staking_count: 0,
        install_upgrade_count: 0,
        standard_count: MAX_STANDARD_COUNT,
        state_validator: |(state, responder)| {
            responder.is_none() && matches!(state, BlockValidationState::InProgress { .. })
        },
    };

    const LESS_THAN_MAX_STANDARD: TestCase = TestCase {
        standard_count: FULL_STANDARD.standard_count - 1,
        state_validator: |(state, responder)| {
            responder.is_none() && matches!(state, BlockValidationState::InProgress { .. })
        },
        ..FULL_STANDARD
    };

    const TOO_MANY_STANDARD: TestCase = TestCase {
        standard_count: FULL_STANDARD.standard_count + 1,
        state_validator: |(state, responder)| {
            responder.is_some() && matches!(state, BlockValidationState::Invalid(_))
        },
        ..FULL_STANDARD
    };

    const FULL_TRANSFER: TestCase = TestCase {
        transfer_count: MAX_TRANSFER_COUNT,
        staking_count: 0,
        install_upgrade_count: 0,
        standard_count: 0,
        state_validator: |(state, responder)| {
            responder.is_none() && matches!(state, BlockValidationState::InProgress { .. })
        },
    };

    const LESS_THAN_MAX_TRANSFER: TestCase = TestCase {
        transfer_count: FULL_TRANSFER.transfer_count - 1,
        state_validator: |(state, responder)| {
            responder.is_none() && matches!(state, BlockValidationState::InProgress { .. })
        },
        ..FULL_TRANSFER
    };

    const TOO_MANY_TRANSFER: TestCase = TestCase {
        transfer_count: FULL_TRANSFER.transfer_count + 1,
        state_validator: |(state, responder)| {
            responder.is_some() && matches!(state, BlockValidationState::Invalid(_))
        },
        ..FULL_TRANSFER
    };

    fn run_test_case(
        TestCase {
            transfer_count,
            staking_count,
            install_upgrade_count,
            standard_count,
            state_validator,
        }: TestCase,
        rng: &mut TestRng,
    ) {
        let mut fixture = Fixture::new(rng);
        let state = fixture.new_state(
            transfer_count,
            staking_count,
            install_upgrade_count,
            standard_count,
        );
        assert!(state_validator(state));
    }

    #[test]
    fn new_state_should_be_valid_with_no_transactions() {
        let mut rng = TestRng::new();
        run_test_case(NO_TRANSACTIONS, &mut rng);
    }

    #[test]
    fn new_state_should_respect_staking_limits() {
        let mut rng = TestRng::new();
        run_test_case(TOO_MANY_STAKING, &mut rng);
        run_test_case(FULL_STAKING, &mut rng);
        run_test_case(LESS_THAN_MAX_STAKING, &mut rng);
    }

    #[test]
    fn new_state_should_respect_install_upgrade_limits() {
        let mut rng = TestRng::new();
        run_test_case(TOO_MANY_INSTALL_UPGRADE, &mut rng);
        run_test_case(FULL_INSTALL_UPGRADE, &mut rng);
        run_test_case(LESS_THAN_MAX_INSTALL_UPGRADE, &mut rng);
    }

    #[test]
    fn new_state_should_respect_standard_limits() {
        let mut rng = TestRng::new();
        run_test_case(TOO_MANY_STANDARD, &mut rng);
        run_test_case(FULL_STANDARD, &mut rng);
        run_test_case(LESS_THAN_MAX_STANDARD, &mut rng);
    }

    #[test]
    fn new_state_should_respect_transfer_limits() {
        let mut rng = TestRng::new();
        run_test_case(TOO_MANY_TRANSFER, &mut rng);
        run_test_case(FULL_TRANSFER, &mut rng);
        run_test_case(LESS_THAN_MAX_TRANSFER, &mut rng);
    }

    #[test]
    fn new_state_should_be_invalid_with_duplicated_transaction() {
        let mut rng = TestRng::new();
        let fixture = Fixture::new(&mut rng);

        let timestamp = Timestamp::from(1000);
        let transfers = vec![new_transfer(fixture.rng, timestamp, TimeDiff::from_millis(200)); 2];

        let transfers_for_block: Vec<Transaction> =
            transfers.iter().map(|transaction| transaction).collect();

        let proposed_block =
            new_proposed_block(timestamp, transfers_for_block, vec![], vec![], vec![]);

        let (state, maybe_responder) = BlockValidationState::new(
            &proposed_block,
            HashSet::new(),
            NodeId::random(fixture.rng),
            new_responder(),
            &fixture.chainspec,
        );

        assert!(matches!(state, BlockValidationState::Invalid(_)));
        assert!(maybe_responder.is_some());
    }

    #[test]
    fn new_state_should_be_in_progress_with_some_transactions() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);

        // This test must generate number of transactions within the limits as per the chainspec.
        let (transfer_count, staking_count, install_upgrade_count, standard_count) = loop {
            let transfer_count = fixture.rng.gen_range(0..10);
            let staking_count = fixture.rng.gen_range(0..20);
            let install_upgrade_count = fixture.rng.gen_range(0..2);
            let standard_count = fixture.rng.gen_range(0..10);
            // Ensure at least one transaction is generated. Otherwise the state will be Valid.
            if transfer_count + staking_count + install_upgrade_count + standard_count > 0 {
                break (
                    transfer_count,
                    staking_count,
                    install_upgrade_count,
                    standard_count,
                );
            }
        };
        let (state, maybe_responder) = fixture.new_state(
            transfer_count,
            staking_count,
            install_upgrade_count,
            standard_count,
        );

        match state {
            BlockValidationState::InProgress {
                missing_transactions,
                holders,
                responders,
                ..
            } => {
                assert_eq!(
                    missing_transactions.len() as u64,
                    standard_count + transfer_count + install_upgrade_count + staking_count
                );
                assert_eq!(holders.len(), 1);
                assert_eq!(holders.values().next().unwrap(), &HolderState::Unasked);
                assert_eq!(responders.len(), 1);
            }
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => {
                panic!("unexpected state")
            }
        }
        assert!(maybe_responder.is_none());
    }

    #[test]
    fn should_add_responder_if_in_progress() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));
        assert_eq!(state.responder_count(), 1);

        let add_responder_result = state.add_responder(new_responder());
        assert!(matches!(add_responder_result, AddResponderResult::Added));
        assert_eq!(state.responder_count(), 2);
    }

    #[test]
    fn should_not_add_responder_if_valid() {
        let mut state = BlockValidationState::Valid(Timestamp::from(1000));
        let add_responder_result = state.add_responder(new_responder());
        assert!(matches!(
            add_responder_result,
            AddResponderResult::ValidationCompleted {
                response_to_send: true,
                ..
            }
        ));
        assert_eq!(state.responder_count(), 0);
    }

    #[test]
    fn should_not_add_responder_if_invalid() {
        let mut state = BlockValidationState::Invalid(Timestamp::from(1000));
        let add_responder_result = state.add_responder(new_responder());
        assert!(matches!(
            add_responder_result,
            AddResponderResult::ValidationCompleted {
                response_to_send: false,
                ..
            }
        ));
        assert_eq!(state.responder_count(), 0);
    }

    #[test]
    fn should_add_new_holder_if_in_progress() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));
        assert_eq!(state.holders_mut().unwrap().len(), 1);

        let new_holder = NodeId::random(fixture.rng);
        state.add_holder(new_holder);
        assert_eq!(state.holders_mut().unwrap().len(), 2);
        assert_eq!(
            state.holders_mut().unwrap().get(&new_holder),
            Some(&HolderState::Unasked)
        );
    }

    #[test]
    fn should_not_change_holder_state() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));
        let (holder, holder_state) = state
            .holders_mut()
            .expect("should have holders")
            .iter_mut()
            .next()
            .expect("should have one entry");
        *holder_state = HolderState::Asked;
        let holder = *holder;

        state.add_holder(holder);
        assert_eq!(state.holders_mut().unwrap().len(), 1);
        assert_eq!(
            state.holders_mut().unwrap().get(&holder),
            Some(&HolderState::Asked)
        );
    }

    #[test]
    fn should_start_fetching() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));
        let (holder, holder_state) = state
            .holders_mut()
            .expect("should have holders")
            .iter_mut()
            .next()
            .expect("should have one entry");
        assert_eq!(*holder_state, HolderState::Unasked);
        let original_holder = *holder;

        // We currently have one unasked holder.  Add some failed holders - should still return
        // `MaybeStartFetching::Start` containing the original holder.
        for _ in 0..3 {
            state
                .holders_mut()
                .unwrap()
                .insert(NodeId::random(fixture.rng), HolderState::Failed);
        }

        let maybe_start_fetching = state.start_fetching();
        match maybe_start_fetching {
            MaybeStartFetching::Start {
                holder,
                missing_transactions,
                ..
            } => {
                assert_eq!(holder, original_holder);
                assert_eq!(missing_transactions.len(), 8);
            }
            _ => panic!("unexpected return value"),
        }

        // The original holder should now be marked as `Asked`.
        let holder_state = state.holders_mut().unwrap().get(&original_holder);
        assert_eq!(holder_state, Some(&HolderState::Asked));
    }

    #[test]
    fn start_fetching_should_return_ongoing_if_any_holder_in_asked_state() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));

        // Change the current (only) holder's state to `Asked`.
        let maybe_start_fetching = state.start_fetching();
        assert!(matches!(
            maybe_start_fetching,
            MaybeStartFetching::Start { .. }
        ));
        let holder_state = state.holders_mut().unwrap().values().next();
        assert_eq!(holder_state, Some(&HolderState::Asked));

        // Add some unasked holders and some failed - should still return
        // `MaybeStartFetching::Ongoing`.
        let unasked_count = fixture.rng.gen_range(0..3);
        for _ in 0..unasked_count {
            state
                .holders_mut()
                .unwrap()
                .insert(NodeId::random(fixture.rng), HolderState::Unasked);
        }
        let failed_count = fixture.rng.gen_range(0..3);
        for _ in 0..failed_count {
            state
                .holders_mut()
                .unwrap()
                .insert(NodeId::random(fixture.rng), HolderState::Failed);
        }

        // Clone the holders collection before calling `start_fetching` as it should be unmodified
        // by the call.
        let holders_before = state.holders_mut().unwrap().clone();

        // `start_fetching` should return `Ongoing` due to the single `Asked` holder.
        let maybe_start_fetching = state.start_fetching();
        assert_eq!(maybe_start_fetching, MaybeStartFetching::Ongoing);

        // The holders should be unchanged.
        assert_eq!(state.holders_mut().unwrap(), &holders_before);
    }

    #[test]
    fn start_fetching_should_return_unable_if_all_holders_in_failed_state() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));

        // Set the original holder's state to `Failed` and add some more failed.
        *state
            .holders_mut()
            .expect("should have holders")
            .values_mut()
            .next()
            .expect("should have one entry") = HolderState::Failed;

        let failed_count = fixture.rng.gen_range(0..3);
        for _ in 0..failed_count {
            state
                .holders_mut()
                .unwrap()
                .insert(NodeId::random(fixture.rng), HolderState::Failed);
        }

        // Clone the holders collection before calling `start_fetching` as it should be unmodified
        // by the call.
        let holders_before = state.holders_mut().unwrap().clone();

        // `start_fetching` should return `Unable` due to no un-failed holders.
        let maybe_start_fetching = state.start_fetching();
        assert_eq!(maybe_start_fetching, MaybeStartFetching::Unable);

        // The holders should be unchanged.
        assert_eq!(state.holders_mut().unwrap(), &holders_before);
    }

    #[test]
    fn start_fetching_should_return_validation_succeeded_if_valid() {
        let mut state = BlockValidationState::Valid(Timestamp::from(1000));
        let maybe_start_fetching = state.start_fetching();
        assert_eq!(
            maybe_start_fetching,
            MaybeStartFetching::ValidationSucceeded
        );
    }

    #[test]
    fn start_fetching_should_return_validation_failed_if_invalid() {
        let mut state = BlockValidationState::Invalid(Timestamp::from(1000));
        let maybe_start_fetching = state.start_fetching();
        assert_eq!(maybe_start_fetching, MaybeStartFetching::ValidationFailed);
    }

    #[test]
    fn state_should_change_to_validation_succeeded() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));

        // While there is still at least one missing transaction, `try_add_transaction_footprint`
        // should keep the state `InProgress` and never return responders.
        let mut footprints = fixture.footprints();
        while footprints.len() > 1 {
            let (transaction_hash, footprint) = footprints.pop().unwrap();
            let responders = state.try_add_transaction_footprint(&transaction_hash, &footprint);
            assert!(responders.is_empty());
            assert!(matches!(
                state,
                BlockValidationState::InProgress { ref responders, .. }
                if !responders.is_empty()
            ));
        }

        // The final transaction should cause the state to go to `Valid` and the responders to be
        // returned.
        let (dt_hash, footprint) = footprints.pop().unwrap();
        let responders = state.try_add_transaction_footprint(&dt_hash, &footprint);
        assert_eq!(responders.len(), 1);
        assert!(matches!(state, BlockValidationState::Valid(_)));
    }

    #[test]
    fn unrelated_transaction_added_should_not_change_state() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        let (appendable_block_before, missing_transactions_before, holders_before) = match &state {
            BlockValidationState::InProgress {
                appendable_block,
                missing_transactions,
                holders,
                ..
            } => (
                appendable_block.clone(),
                missing_transactions.clone(),
                holders.clone(),
            ),
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => {
                panic!("unexpected state")
            }
        };

        // Create a new, random transaction.
        let transaction = new_standard(fixture.rng, 1500.into(), TimeDiff::from_seconds(1));
        let transaction_hash = match &transaction {
            Transaction::Deploy(deploy) => TransactionHash::Deploy(*deploy.hash()),
            Transaction::V1(v1) => TransactionHash::V1(*v1.hash()),
        };
        let chainspec = Chainspec::default();
        let footprint = transaction.footprint(&chainspec).unwrap();

        // Ensure trying to add it doesn't change the state.
        let responders = state.try_add_transaction_footprint(&transaction_hash, &footprint);
        assert!(responders.is_empty());
        match &state {
            BlockValidationState::InProgress {
                appendable_block,
                missing_transactions: missing_deploys,
                holders,
                ..
            } => {
                assert_eq!(&appendable_block_before, appendable_block);
                assert_eq!(&missing_transactions_before, missing_deploys);
                assert_eq!(&holders_before, holders);
            }
            BlockValidationState::Valid(_) | BlockValidationState::Invalid(_) => {
                panic!("unexpected state")
            }
        };
    }

    #[test]
    fn state_should_change_to_validation_failed() {
        let mut rng = TestRng::new();
        let mut fixture = Fixture::new(&mut rng);
        // Add an invalid (future-dated) transaction to the fixture.
        let invalid_transaction =
            new_standard(fixture.rng, Timestamp::MAX, TimeDiff::from_seconds(1));
        let invalid_transaction_hash = invalid_transaction.hash();
        fixture.transactions.push(invalid_transaction.clone());
        let (mut state, _maybe_responder) = fixture.new_state(2, 2, 2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));

        // Add some valid deploys, should keep the state `InProgress` and never return responders.
        let mut footprints = fixture.footprints();
        while footprints.len() > 3 {
            let (dt_hash, footprint) = footprints.pop().unwrap();
            if dt_hash.transaction_hash() == invalid_transaction_hash {
                continue;
            }
            let responders = state.try_add_transaction_footprint(&dt_hash, &footprint);
            assert!(responders.is_empty());
        }

        let transaction_hash = invalid_transaction.hash();
        // The invalid transaction should cause the state to go to `Invalid` and the responders to
        // be returned.
        let chainspec = Chainspec::default();
        let footprint = invalid_transaction.footprint(&chainspec).unwrap();
        let responders = state.try_add_transaction_footprint(&transaction_hash, &footprint);
        assert_eq!(responders.len(), 1);
        assert!(matches!(state, BlockValidationState::Invalid(_)));
    }
}
