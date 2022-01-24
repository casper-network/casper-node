use crate::{
    effect::{EffectBuilder, EffectExt, Effects},
    types::{Block, NodeId},
};

use super::{event::DeploysResult, Event, ReactorEventT};

pub(super) fn fetch_block_deploys<REv>(
    effect_builder: EffectBuilder<REv>,
    peer: NodeId,
    block: Block,
) -> Effects<Event>
where
    REv: ReactorEventT,
{
    effect_builder
        .validate_block(peer.clone(), block.clone())
        .event(move |valid| {
            if valid {
                Event::GetDeploysResult(DeploysResult::Found(Box::new(block)))
            } else {
                Event::GetDeploysResult(DeploysResult::NotFound(Box::new(block), peer))
            }
        })
}
