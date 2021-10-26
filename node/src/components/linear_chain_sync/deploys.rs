use crate::{
    effect::{EffectBuilder, EffectExt, Effects},
    types::Block,
};

use super::{event::DeploysResult, Event, ReactorEventT};

pub(super) fn fetch_block_deploys<I: Clone + Send + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block: Block,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
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
