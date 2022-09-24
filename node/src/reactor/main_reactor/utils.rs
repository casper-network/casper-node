use crate::{
    components::{blocks_accumulator::SyncInstruction, sync_leaper, InitializedComponent},
    effect::{EffectBuilder, EffectExt, Effects},
    reactor::main_reactor::{MainEvent, MainReactor},
    types::{ActivationPoint, Block, BlockHash, Chainspec, ChainspecRawBytes, NodeId},
};
use casper_execution_engine::core::engine_state::{ChainspecRegistry, UpgradeConfig};
use casper_hashing::Digest;
use casper_types::{EraId, Key, ProtocolVersion, StoredValue, Timestamp};
use std::{borrow::Borrow, collections::BTreeMap, sync::Arc};

pub(super) fn enqueue_shutdown<T: ToString + Send + 'static>(message: T) -> Effects<MainEvent> {
    async {}.event(move |_| MainEvent::Shutdown(message.to_string()))
}

pub(super) fn initialize_component(
    effect_builder: EffectBuilder<MainEvent>,
    component: &mut impl InitializedComponent<MainEvent>,
    component_name: &str,
    initiating_event: MainEvent,
) -> Option<Effects<MainEvent>> {
    if component.is_uninitialized() {
        let mut effects = effect_builder.immediately().event(|()| initiating_event);
        effects.extend(
            effect_builder
                .immediately()
                .event(|()| MainEvent::ReactorCrank),
        );
        return Some(effects);
    }
    let component_name = component_name.to_string();
    if component.is_fatal() {
        return Some(effect_builder.immediately().event(move |()| {
            MainEvent::Shutdown(format!("{} failed to initialize", component_name))
        }));
    }
    None
}
