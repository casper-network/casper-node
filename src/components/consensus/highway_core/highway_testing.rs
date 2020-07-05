use super::{finality_detector::FinalityDetector, highway::Highway, vertex::Vertex};
use crate::components::consensus::{
    consensus_des_testing::{ConsensusInstance, Instant, TargetedMessage, ValidatorId},
    traits::Context,
};

struct HighwayConsensus<C: Context> {
    highway: Highway<C>,
    finality_detector: FinalityDetector<C>,
}

#[derive(Clone, Debug)]
enum HighwayMessage<C: Context> {
    Timer(Instant),
    NewVertex(Vertex<C>),
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
        todo!()
    }
}
