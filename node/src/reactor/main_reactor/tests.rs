mod auction;
mod binary_port;
mod configs_override;
mod consensus_rules;
mod fixture;
mod gas_price;
mod initial_stakes;
mod network_general;
mod rewards;
mod switch_blocks;
mod transaction_scenario;
mod transactions;

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use num_rational::Ratio;
use tracing::info;

use casper_storage::{
    data_access_layer::{
        balance::{BalanceHandling, BalanceResult},
        BalanceRequest, BidsRequest, TotalSupplyRequest, TotalSupplyResult,
    },
    global_state::state::StateProvider,
};
use casper_types::{
    execution::ExecutionResult, system::auction::BidKind, testing::TestRng, Chainspec, Deploy,
    EraId, FeeHandling, Gas, HoldBalanceHandling, Key, PricingHandling, PricingMode, PublicKey,
    RefundHandling, SecretKey, StoredValue, TimeDiff, Timestamp, Transaction, TransactionHash,
    U512,
};

use crate::{
    components::consensus::{ClContext, ConsensusMessage, HighwayMessage, HighwayVertex},
    effect::incoming::ConsensusMessageIncoming,
    reactor::{
        main_reactor::{MainEvent, MainReactor},
        Runner,
    },
    testing::{self, filter_reactor::FilterReactor, ConditionCheckReactor},
    types::{transaction::transaction_v1_builder::TransactionV1Builder, NodeId},
    utils::RESOURCES_PATH,
};

const ERA_ZERO: EraId = EraId::new(0);
const ERA_ONE: EraId = EraId::new(1);
const ERA_TWO: EraId = EraId::new(2);
const ERA_THREE: EraId = EraId::new(3);
const TEN_SECS: Duration = Duration::from_secs(10);
const THIRTY_SECS: Duration = Duration::from_secs(30);
const ONE_MIN: Duration = Duration::from_secs(60);
const TWO_MIN: Duration = Duration::from_secs(120);

type Nodes = testing::network::Nodes<FilterReactor<MainReactor>>;

impl Runner<ConditionCheckReactor<FilterReactor<MainReactor>>> {
    fn main_reactor(&self) -> &MainReactor {
        self.reactor().inner().inner()
    }

    fn main_reactor_as_mut(&mut self) -> &mut MainReactor {
        self.reactor.inner_mut().inner_mut()
    }
}

/// Given a block height and a node id, returns a predicate to check if the lowest available block
/// for the specified node is at or below the specified height.
fn node_has_lowest_available_block_at_or_below_height(
    height: u64,
    node_id: NodeId,
) -> impl Fn(&Nodes) -> bool {
    move |nodes: &Nodes| {
        nodes.get(&node_id).is_none_or(|runner| {
            let available_block_range = runner.main_reactor().storage().get_available_block_range();
            if available_block_range.low() == 0 && available_block_range.high() == 0 {
                false
            } else {
                available_block_range.low() <= height
            }
        })
    }
}

fn is_ping(event: &MainEvent) -> bool {
    if let MainEvent::ConsensusMessageIncoming(ConsensusMessageIncoming { message, .. }) = event {
        if let ConsensusMessage::Protocol { ref payload, .. } = **message {
            return matches!(
                payload.deserialize_incoming::<HighwayMessage<ClContext>>(),
                Ok(HighwayMessage::<ClContext>::NewVertex(HighwayVertex::Ping(
                    _
                )))
            );
        }
    }
    false
}
