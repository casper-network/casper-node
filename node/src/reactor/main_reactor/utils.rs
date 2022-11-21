use futures::FutureExt;
use smallvec::smallvec;
use tracing::info;

use crate::{
    components::InitializedComponent,
    effect::{EffectBuilder, EffectExt, Effects},
    fatal,
    reactor::main_reactor::MainEvent,
};

pub(super) fn initialize_component(
    effect_builder: EffectBuilder<MainEvent>,
    component: &mut impl InitializedComponent<MainEvent>,
    initiating_event: MainEvent,
) -> Option<Effects<MainEvent>> {
    if component.is_uninitialized() {
        info!("initialized {}", component.name());
        return Some(smallvec![async { smallvec![initiating_event] }.boxed()]);
    }
    if component.is_fatal() {
        return Some(fatal!(effect_builder, "{} failed to initialize", component.name()).ignore());
    }
    None
}
