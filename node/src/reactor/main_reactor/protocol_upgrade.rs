use std::{sync::Arc, time::Duration};
use tokio::runtime::Handle;
use tracing::info;

use casper_types::{
    BlockHash, BlockHeader, Chainspec, ChainspecRawBytes, Digest, EraId, TimeDiff, Timestamp,
};
use datasize::DataSize;

use crate::{components::contract_runtime::ContractRuntime, reactor::main_reactor::Error};

/// The information needed to produce the deterministic "immediate switch block" following a
/// protocol upgrade, once the node is ready to sign and gossip it.
#[derive(Clone, DataSize, Debug)]
pub(super) struct PendingImmediateSwitchBlock {
    pub(super) next_block_height: u64,
    #[data_size(skip)]
    pub(super) post_state_hash: Digest,
    pub(super) parent_hash: BlockHash,
    pub(super) parent_seed: Digest,
    pub(super) era_id: EraId,
    pub(super) timestamp: Timestamp,
}

impl PendingImmediateSwitchBlock {
    pub(super) fn next_block_height(&self) -> u64 {
        self.next_block_height
    }

    pub(super) fn post_state_hash(&self) -> Digest {
        self.post_state_hash
    }

    pub(super) fn parent_hash(&self) -> BlockHash {
        self.parent_hash
    }

    pub(super) fn parent_seed(&self) -> Digest {
        self.parent_seed
    }

    pub(super) fn era_id(&self) -> EraId {
        self.era_id
    }

    pub(super) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }
}

/// If `tip_header` is a switch block that is the last block before the chainspec's
/// activation point, synchronously commits the protocol upgrade against `contract_runtime`'s
/// global state. Returns the info needed to later produce, sign, and gossip the resulting
/// immediate switch block, once the reactor is ready to do so (see
/// [`Self::maybe_finish_pending_upgrade`]). Returns `Ok(None)` if no upgrade is due
///
/// This is an associated function (rather than a `&self` method) so it can be called from
/// `MainReactor::new`, before the reactor itself has been constructed -- that's the only
/// call site: a fresh restart whose local tip already sits at the pre-activation switch
/// block (e.g. after a live node shuts itself down for the upgrade). A node still *catching
/// up* through a historical activation point does not go through here; it just receives the
/// post-upgrade chain via the ordinary block-synchronizer fetch path, like any other
/// historical data.
pub(super) fn commit_upgrade_if_needed(
    contract_runtime: &ContractRuntime,
    chainspec: &Arc<Chainspec>,
    chainspec_raw_bytes: &Arc<ChainspecRawBytes>,
    tip_header: Option<&BlockHeader>,
    upgrade_timeout: TimeDiff,
) -> Result<Option<PendingImmediateSwitchBlock>, Error> {
    let Some(tip_header) = tip_header else {
        return Ok(None);
    };
    if !(tip_header.is_switch_block()
        && tip_header.is_last_block_before_activation(&chainspec.protocol_config))
    {
        return Ok(None);
    }

    info!(
        era_id = %tip_header.era_id(),
        height = tip_header.height(),
        "committing protocol upgrade"
    );

    let upgrade_config = chainspec
        .upgrade_config_from_parts(
            *tip_header.state_root_hash(),
            tip_header.protocol_version(),
            chainspec.protocol_config.activation_point.era_id(),
            chainspec_raw_bytes.clone(),
        )
        .map_err(Error::ProtocolUpgrade)?;

    // Executing protocol upgrade can be time consuming. It's executed in the background so the
    // upgrade_timeout can be enforced. This function stays synchronous -- it's called
    // from `MainReactor::new`, before the reactor's async event loop exists -- so the
    // wait for that bounded future to resolve is bridged onto a dedicated scoped
    // thread, which calls `Handle::block_on` directly.
    let handle = Handle::current();
    let post_state_hash = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                handle.block_on(async {
                    match tokio::time::timeout(
                        Duration::from(upgrade_timeout),
                        contract_runtime.commit_protocol_upgrade(upgrade_config),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => Err(format!(
                            "protocol upgrade did not complete within {}",
                            upgrade_timeout
                        )),
                    }
                })
            })
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
    .map_err(Error::ProtocolUpgrade)?;

    Ok(Some(PendingImmediateSwitchBlock {
        next_block_height: tip_header.height() + 1,
        post_state_hash,
        parent_hash: tip_header.block_hash(),
        parent_seed: *tip_header.accumulated_seed(),
        era_id: tip_header.next_block_era_id(),
        // Adding one second here to make sure the timestamp is monotonically growing -
        // it's important for EVM smart contracts
        timestamp: tip_header
            .timestamp()
            .saturating_add(TimeDiff::from_seconds(1)),
    }))
}
