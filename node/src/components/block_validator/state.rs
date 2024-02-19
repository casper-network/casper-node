use std::{
    collections::{hash_map::Entry, BTreeSet, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    iter, mem,
};

use datasize::DataSize;
use tracing::{debug, error, warn};

use casper_types::{
    Chainspec, DeployApprovalsHash, FinalitySignatureId, Timestamp, TransactionApproval,
    TransactionApprovalsHash, TransactionFootprint, TransactionHash, TransactionV1ApprovalsHash,
};

use crate::{
    components::consensus::{ClContext, ProposedBlock},
    effect::Responder,
    types::{
        appendable_block::AppendableBlock, DeployHashWithApprovals, DeployOrTransactionHash,
        DeployOrTransferHash, NodeId, TransactionHashWithApprovals, TransactionV1OrTransferV1Hash,
    },
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
        missing_transactions: HashMap<DeployOrTransactionHash, TransactionApprovalsHash>,
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
    approvals: BTreeSet<TransactionApproval>,
    approvals_hash: TransactionApprovalsHash,
}

impl ApprovalInfo {
    fn new(
        approvals: BTreeSet<TransactionApproval>,
        approvals_hash: TransactionApprovalsHash,
    ) -> Self {
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
        missing_transactions: HashMap<DeployOrTransactionHash, ApprovalInfo>,
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
        block: &ProposedBlock<ClContext>,
        missing_signatures: HashSet<FinalitySignatureId>,
        sender: NodeId,
        responder: Responder<bool>,
        chainspec: &Chainspec,
    ) -> (Self, Option<Responder<bool>>) {
        let transaction_count = block.transactions().len() + block.transfers().len();
        if transaction_count == 0 {
            let state = BlockValidationState::Valid(block.timestamp());
            return (state, Some(responder));
        }

        if block.transactions().len()
            > chainspec.transaction_config.block_max_standard_count as usize
        {
            warn!("too many non-transfer transactions");
            let state = BlockValidationState::Invalid(block.timestamp());
            return (state, Some(responder));
        }
        if block.transfers().len() > chainspec.transaction_config.block_max_transfer_count as usize
        {
            warn!("too many transfers");
            let state = BlockValidationState::Invalid(block.timestamp());
            return (state, Some(responder));
        }

        let appendable_block =
            AppendableBlock::new(chainspec.transaction_config, block.timestamp());

        let mut missing_transactions = HashMap::new();
        let transactions_iter = block.transactions().into_iter().map(|dhwa| {
            let dt_hash = match &dhwa {
                TransactionHashWithApprovals::Deploy { deploy_hash, .. } => {
                    DeployOrTransactionHash::from(DeployOrTransferHash::Deploy(*deploy_hash))
                }
                TransactionHashWithApprovals::V1(thwa) => DeployOrTransactionHash::from(
                    TransactionV1OrTransferV1Hash::Transaction(*thwa.transaction_hash()),
                ),
            };
            (dt_hash, dhwa.approvals())
        });
        let transfers_iter = block.transfers().into_iter().map(|dhwa| {
            let dt_hash = match &dhwa {
                TransactionHashWithApprovals::Deploy { deploy_hash, .. } => {
                    DeployOrTransactionHash::from(DeployOrTransferHash::Transfer(*deploy_hash))
                }
                TransactionHashWithApprovals::V1(thwa) => DeployOrTransactionHash::from(
                    TransactionV1OrTransferV1Hash::Transfer(*thwa.transaction_hash()),
                ),
            };
            (dt_hash, dhwa.approvals())
        });
        for (dt_hash, approvals) in transactions_iter.chain(transfers_iter) {
            let approval_info: ApprovalInfo = match dt_hash {
                DeployOrTransactionHash::Deploy(_) => {
                    let deploy_approvals: BTreeSet<_> = approvals
                        .iter()
                        .cloned()
                        .flat_map(|transaction_approval| match transaction_approval {
                            TransactionApproval::Deploy(deploy_approval) => Some(deploy_approval),
                            TransactionApproval::V1(_) => {
                                error!(%dt_hash, "unexpected V1 approval on legacy deploy");
                                None
                            }
                        })
                        .collect();
                    match DeployApprovalsHash::compute(&deploy_approvals) {
                        Ok(approvals_hash) => ApprovalInfo::new(approvals, approvals_hash.into()),
                        Err(error) => {
                            warn!(%dt_hash, %error, "could not compute approvals hash");
                            let state = BlockValidationState::Invalid(block.timestamp());
                            return (state, Some(responder));
                        }
                    }
                }
                DeployOrTransactionHash::V1(_) => {
                    let transaction_v1_approvals: BTreeSet<_> = approvals
                        .iter()
                        .cloned()
                        .flat_map(|transaction_approval| match transaction_approval {
                            TransactionApproval::Deploy(_) => {
                                error!(%dt_hash,"unexpected legacy deploy approval on V1 transaction");
                                None
                            }
                            TransactionApproval::V1(transaction_v1_approval) => {
                                Some(transaction_v1_approval)
                            }
                        })
                        .collect();
                    match TransactionV1ApprovalsHash::compute(&transaction_v1_approvals) {
                        Ok(approvals_hash) => ApprovalInfo::new(approvals, approvals_hash.into()),
                        Err(error) => {
                            warn!(%dt_hash, %error, "could not compute approvals hash");
                            let state = BlockValidationState::Invalid(block.timestamp());
                            return (state, Some(responder));
                        }
                    }
                }
            };

            if missing_transactions
                .insert(dt_hash, approval_info)
                .is_some()
            {
                warn!(%dt_hash, "duplicated transaction in proposed block");
                let state = BlockValidationState::Invalid(block.timestamp());
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
        dt_hash: &DeployOrTransactionHash,
        footprint: &TransactionFootprint,
    ) -> Vec<Responder<bool>> {
        let (new_state, responders) = match self {
            BlockValidationState::InProgress {
                appendable_block,
                missing_transactions,
                responders,
                ..
            } => {
                let approvals_info = match missing_transactions.remove(dt_hash) {
                    Some(info) => info,
                    None => {
                        // If this deploy is not present, just return.
                        return vec![];
                    }
                };
                // Try adding the footprint to the appendable block to see if the block remains
                // valid.
                let transaction_hash: TransactionHash = dt_hash.transaction_hash();
                let dhwa = TransactionHashWithApprovals::new_from_hash_and_approvals(
                    &transaction_hash,
                    &approvals_info.approvals,
                );

                let add_result = match (dt_hash, dhwa) {
                    (
                        DeployOrTransactionHash::Deploy(_),
                        TransactionHashWithApprovals::Deploy {
                            deploy_hash,
                            approvals,
                        },
                    ) => {
                        let TransactionFootprint::Deploy(deploy_footprint) = footprint else {
                            error!(%dt_hash, "legacy deploy with transaction V1 footprint");
                            return vec![];
                        };
                        let dhwa = DeployHashWithApprovals::new(deploy_hash, approvals);
                        appendable_block.add_deploy(dhwa, deploy_footprint)
                    }
                    (
                        DeployOrTransactionHash::V1(_),
                        TransactionHashWithApprovals::V1(transaction_v1_hash_with_approvals),
                    ) => {
                        let TransactionFootprint::V1(transaction_v1_footprint) = footprint else {
                            error!(%dt_hash, "transaction v1 with legacy deploy footprint");
                            return vec![];
                        };
                        appendable_block.add_transaction_v1(
                            transaction_v1_hash_with_approvals,
                            transaction_v1_footprint,
                        )
                    }
                    (DeployOrTransactionHash::Deploy(_), TransactionHashWithApprovals::V1(_)) => {
                        error!(%dt_hash, "legacy deploy with transaction V1 approvals");
                        return vec![];
                    }
                    (
                        DeployOrTransactionHash::V1(_),
                        TransactionHashWithApprovals::Deploy { .. },
                    ) => {
                        error!(%dt_hash, "transaction V1 with legacy deploy approvals");
                        return vec![];
                    }
                };

                match add_result {
                    Ok(()) => {
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
                        warn!(%dt_hash, ?footprint, %error, "block invalid");
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
        dt_hash: &DeployOrTransactionHash,
    ) -> Vec<Responder<bool>> {
        let (timestamp, responders) = match self {
            BlockValidationState::InProgress {
                appendable_block,
                missing_transactions,
                responders,
                ..
            } => {
                if !missing_transactions.contains_key(dt_hash) {
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
                    DeployOrTransactionHash::Deploy(deploy) => deploy.deploy_hash().into(),
                    DeployOrTransactionHash::V1(v1) => (*v1).into(),
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

    use casper_types::{testing::TestRng, ChainspecRawBytes, TimeDiff, Transaction};

    use super::{super::tests::*, *};
    use crate::{types::TransactionHashWithApprovals, utils::Loadable};

    struct Fixture {
        rng: TestRng,
        transactions: Vec<Transaction>,
        transfers: Vec<Transaction>,
        chainspec: Chainspec,
    }

    impl Fixture {
        fn new() -> Self {
            let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
            Fixture {
                rng: TestRng::new(),
                transactions: vec![],
                transfers: vec![],
                chainspec,
            }
        }

        /// Returns a new `BlockValidationState` with the specified number of transactions and
        /// transfers added to any pre-existing ones in the fixture.
        fn new_state(
            &mut self,
            transaction_count: u64,
            transfer_count: u64,
        ) -> (BlockValidationState, Option<Responder<bool>>) {
            let ttl = TimeDiff::from_seconds(10);
            let transactions: Vec<_> = (0..transaction_count)
                .map(|index| new_non_transfer(&mut self.rng, Timestamp::from(1000 + index), ttl))
                .collect();
            self.transactions.extend(transactions);
            let transfers: Vec<_> = (0..transfer_count)
                .map(|index| {
                    new_transfer(
                        &mut self.rng,
                        Timestamp::from(1000 + transaction_count + index),
                        ttl,
                    )
                })
                .collect();
            self.transfers.extend(transfers);

            let transactions_for_block = self
                .transactions
                .iter()
                .map(|transaction| {
                    TransactionHashWithApprovals::new_from_hash_and_approvals(
                        &transaction.hash(),
                        &transaction.approvals(),
                    )
                })
                .collect();
            let transfers_for_block = self
                .transfers
                .iter()
                .map(|transfer| {
                    TransactionHashWithApprovals::new_from_hash_and_approvals(
                        &transfer.hash(),
                        &transfer.approvals(),
                    )
                })
                .collect();

            let proposed_block = new_proposed_block(
                Timestamp::from(1000 + transaction_count + transfer_count),
                transfers_for_block,
                vec![],
                vec![],
                transactions_for_block,
            );

            BlockValidationState::new(
                &proposed_block,
                HashSet::new(),
                NodeId::random(&mut self.rng),
                new_responder(),
                &self.chainspec,
            )
        }

        fn footprints(&self) -> Vec<(DeployOrTransactionHash, TransactionFootprint)> {
            self.transactions
                .iter()
                .map(|transaction| match transaction {
                    Transaction::Deploy(deploy) => {
                        let hash = deploy.hash();
                        let footprint = deploy.footprint().unwrap();
                        if footprint.is_transfer {
                            panic!("unexpected transfer in transactions");
                        } else {
                            (
                                DeployOrTransactionHash::Deploy(DeployOrTransferHash::Deploy(
                                    *hash,
                                )),
                                TransactionFootprint::from(footprint),
                            )
                        }
                    }
                    Transaction::V1(v1) => {
                        let hash = v1.hash();
                        let footprint = v1.footprint().unwrap();
                        if footprint.is_transfer() {
                            panic!("unexpected transfer in transactions");
                        } else {
                            (
                                DeployOrTransactionHash::V1(
                                    TransactionV1OrTransferV1Hash::Transaction(*hash),
                                ),
                                TransactionFootprint::from(footprint),
                            )
                        }
                    }
                })
                .chain(self.transfers.iter().map(|transfer| match transfer {
                    Transaction::Deploy(deploy) => {
                        let hash = deploy.hash();
                        let footprint = deploy.footprint().unwrap();
                        if footprint.is_transfer {
                            (
                                DeployOrTransactionHash::Deploy(DeployOrTransferHash::Transfer(
                                    *hash,
                                )),
                                TransactionFootprint::from(footprint),
                            )
                        } else {
                            panic!("unexpected transaction in transfers");
                        }
                    }
                    Transaction::V1(v1) => {
                        let hash = v1.hash();
                        let footprint = v1.footprint().unwrap();
                        if footprint.is_transfer() {
                            (
                                DeployOrTransactionHash::V1(
                                    TransactionV1OrTransferV1Hash::Transfer(*hash),
                                ),
                                TransactionFootprint::from(footprint),
                            )
                        } else {
                            panic!("unexpected transaction in transfers");
                        }
                    }
                }))
                .collect()
        }
    }

    fn new_responder() -> Responder<bool> {
        let (sender, _receiver) = oneshot::channel();
        Responder::without_shutdown(sender)
    }

    #[test]
    fn new_state_should_be_valid_with_no_transactions() {
        let mut fixture = Fixture::new();
        let (state, maybe_responder) = fixture.new_state(0, 0);
        assert!(matches!(state, BlockValidationState::Valid(_)));
        assert!(maybe_responder.is_some());
    }

    #[test]
    fn new_state_should_be_invalid_with_too_many_transactions() {
        let mut fixture = Fixture::new();
        let transaction_count = 5_u64;
        fixture
            .chainspec
            .transaction_config
            .block_max_standard_count = transaction_count as u32 - 1;
        let (state, maybe_responder) = fixture.new_state(transaction_count, 0);
        assert!(matches!(state, BlockValidationState::Invalid(_)));
        assert!(maybe_responder.is_some());
    }

    #[test]
    fn new_state_should_be_invalid_with_too_many_transfers() {
        let mut fixture = Fixture::new();
        let transfer_count = 5_u64;
        fixture
            .chainspec
            .transaction_config
            .block_max_transfer_count = transfer_count as u32 - 1;
        let (state, maybe_responder) = fixture.new_state(0, transfer_count);
        assert!(matches!(state, BlockValidationState::Invalid(_)));
        assert!(maybe_responder.is_some());
    }

    #[test]
    fn new_state_should_be_invalid_with_duplicated_transaction() {
        let mut fixture = Fixture::new();

        let timestamp = Timestamp::from(1000);
        let transfers =
            vec![new_transfer(&mut fixture.rng, timestamp, TimeDiff::from_millis(200)); 2];

        let transfers_for_block = transfers
            .iter()
            .map(TransactionHashWithApprovals::from)
            .collect();

        let proposed_block =
            new_proposed_block(timestamp, transfers_for_block, vec![], vec![], vec![]);

        let (state, maybe_responder) = BlockValidationState::new(
            &proposed_block,
            HashSet::new(),
            NodeId::random(&mut fixture.rng),
            new_responder(),
            &fixture.chainspec,
        );

        assert!(matches!(state, BlockValidationState::Invalid(_)));
        assert!(maybe_responder.is_some());
    }

    #[test]
    fn new_state_should_be_in_progress_with_some_transactions() {
        let mut fixture = Fixture::new();
        let transaction_count = fixture.rng.gen_range(1..10);
        let transfer_count = fixture.rng.gen_range(0..10);
        let (state, maybe_responder) = fixture.new_state(transaction_count, transfer_count);

        match state {
            BlockValidationState::InProgress {
                missing_transactions,
                holders,
                responders,
                ..
            } => {
                assert_eq!(
                    missing_transactions.len() as u64,
                    transaction_count + transfer_count
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
        let mut fixture = Fixture::new();
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
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
        let mut fixture = Fixture::new();
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));
        assert_eq!(state.holders_mut().unwrap().len(), 1);

        let new_holder = NodeId::random(&mut fixture.rng);
        state.add_holder(new_holder);
        assert_eq!(state.holders_mut().unwrap().len(), 2);
        assert_eq!(
            state.holders_mut().unwrap().get(&new_holder),
            Some(&HolderState::Unasked)
        );
    }

    #[test]
    fn should_not_change_holder_state() {
        let mut fixture = Fixture::new();
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
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
        let mut fixture = Fixture::new();
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
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
                .insert(NodeId::random(&mut fixture.rng), HolderState::Failed);
        }

        let maybe_start_fetching = state.start_fetching();
        match maybe_start_fetching {
            MaybeStartFetching::Start {
                holder,
                missing_transactions,
                ..
            } => {
                assert_eq!(holder, original_holder);
                assert_eq!(missing_transactions.len(), 4);
            }
            _ => panic!("unexpected return value"),
        }

        // The original holder should now be marked as `Asked`.
        let holder_state = state.holders_mut().unwrap().get(&original_holder);
        assert_eq!(holder_state, Some(&HolderState::Asked));
    }

    #[test]
    fn start_fetching_should_return_ongoing_if_any_holder_in_asked_state() {
        let mut fixture = Fixture::new();
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
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
                .insert(NodeId::random(&mut fixture.rng), HolderState::Unasked);
        }
        let failed_count = fixture.rng.gen_range(0..3);
        for _ in 0..failed_count {
            state
                .holders_mut()
                .unwrap()
                .insert(NodeId::random(&mut fixture.rng), HolderState::Failed);
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
        let mut fixture = Fixture::new();
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
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
                .insert(NodeId::random(&mut fixture.rng), HolderState::Failed);
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
        let mut fixture = Fixture::new();
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));

        // While there is still at least one missing transaction, `try_add_transaction_footprint`
        // should keep the state `InProgress` and never return responders.
        let mut footprints = fixture.footprints();
        while footprints.len() > 1 {
            let (dt_hash, footprint) = footprints.pop().unwrap();
            let responders = state.try_add_transaction_footprint(&dt_hash, &footprint);
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
        let mut fixture = Fixture::new();
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
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
        let deploy = new_legacy_deploy(&mut fixture.rng, 1500.into(), TimeDiff::from_seconds(1));
        let dt_hash = DeployOrTransferHash::Deploy(*deploy.hash()).into();
        let footprint = deploy.footprint().unwrap().into();

        // Ensure trying to add it doesn't change the state.
        let responders = state.try_add_transaction_footprint(&dt_hash, &footprint);
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
        let mut fixture = Fixture::new();
        // Add an invalid (future-dated) transaction to the fixture.
        let invalid_deploy =
            new_legacy_deploy(&mut fixture.rng, Timestamp::MAX, TimeDiff::from_seconds(1));
        let invalid_deploy_hash = *invalid_deploy.hash();
        let invalid_deploy = Transaction::from(invalid_deploy);
        fixture.transactions.push(invalid_deploy.clone());
        let (mut state, _maybe_responder) = fixture.new_state(2, 2);
        assert!(matches!(state, BlockValidationState::InProgress { .. }));

        // Add some valid deploys, should keep the state `InProgress` and never return responders.
        let mut footprints = fixture.footprints();
        while footprints.len() > 3 {
            let (dt_hash, footprint) = footprints.pop().unwrap();
            if dt_hash.transaction_hash() == invalid_deploy_hash.into() {
                continue;
            }
            let responders = state.try_add_transaction_footprint(&dt_hash, &footprint);
            assert!(responders.is_empty());
        }

        // The invalid deploy should cause the state to go to `Invalid` and the responders to be
        // returned.
        let dt_hash =
            DeployOrTransactionHash::from(DeployOrTransferHash::Deploy(invalid_deploy_hash));
        let footprint = invalid_deploy.footprint().unwrap();
        let responders = state.try_add_transaction_footprint(&dt_hash, &footprint);
        assert_eq!(responders.len(), 1);
        assert!(matches!(state, BlockValidationState::Invalid(_)));
    }
}
