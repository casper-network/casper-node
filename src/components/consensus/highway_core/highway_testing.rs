use super::{
    evidence::Evidence,
    finality_detector::{FinalityDetector, FinalityResult},
    highway::Highway,
    state::State,
    vertex::Vertex,
};
use crate::components::consensus::{
    consensus_des_testing::{
        ConsensusInstance, DeliverySchedule, Instant, Strategy, TargetedMessage, TestHarness,
        ValidatorId,
    },
    traits::Context,
    BlockContext,
};

struct HighwayConsensus<C: Context> {
    highway: Highway<C>,
    finality_detector: FinalityDetector<C>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum HighwayMessage<C: Context> {
    Timer(Instant),
    NewVertex(Vertex<C>),
    RequestBlock(BlockContext),
}

use HighwayMessage::*;

impl<C: Context> PartialOrd for HighwayMessage<C> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(&other))
    }
}

impl<C: Context> Ord for HighwayMessage<C> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Timer(_), Timer(_)) => self.cmp(&other),
            (Timer(_), _) => std::cmp::Ordering::Less,
            (NewVertex(v1), NewVertex(v2)) => match (v1, v2) {
                (Vertex::Vote(swv1), Vertex::Vote(swv2)) => swv1.hash().cmp(&swv2.hash()),
                (Vertex::Vote(_), _) => std::cmp::Ordering::Less,
                (
                    Vertex::Evidence(Evidence::Equivocation(ev1_a, ev1_b)),
                    Vertex::Evidence(Evidence::Equivocation(ev2_a, ev2_b)),
                ) => ev1_a
                    .hash()
                    .cmp(&ev2_a.hash())
                    .then_with(|| ev1_b.hash().cmp(&ev2_b.hash())),
                (Vertex::Evidence(_), _) => std::cmp::Ordering::Less,
            },
            (NewVertex(_), _) => std::cmp::Ordering::Less,
            (RequestBlock(_), RequestBlock(_)) => self.cmp(&other),
            (RequestBlock(_), _) => std::cmp::Ordering::Less,
        }
    }
}

impl<C: Context> ConsensusInstance<C::ConsensusValue> for HighwayConsensus<C> {
    type In = HighwayMessage<C>;
    type Out = HighwayMessage<C>;

    fn handle_message(
        &mut self,
        sender: ValidatorId,
        m: Self::In,
        is_faulty: bool,
    ) -> (Vec<C::ConsensusValue>, Vec<TargetedMessage<Self::Out>>) {
        let messages = todo!();

        let finality_results = run_finalizer(&mut self.finality_detector, self.highway.state());

        (finality_results, messages)
    }
}

/// Runs a finality detector and returns the result.
/// TODO: Handle equivocations and exceeded FTT.
fn run_finalizer<C: Context>(
    finality_detector: &mut FinalityDetector<C>,
    state: &State<C>,
) -> Vec<C::ConsensusValue> {
    match finality_detector.run(state) {
        FinalityResult::Finalized(v, _equivocated_ids) => vec![v],
        FinalityResult::None => vec![],
        FinalityResult::FttExceeded => unimplemented!(),
    }
}

struct HighwayTestHarness<C, DS, R>
where
    C: Context,
    DS: Strategy<DeliverySchedule>,
{
    test_harness: TestHarness<HighwayMessage<C>, C::ConsensusValue, HighwayConsensus<C>, DS, R>,
    start_time: u64,
    rand: R,
}
