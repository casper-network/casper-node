use crate::{
    components::InitializedComponent,
    effect::{EffectBuilder, EffectExt, Effects},
    reactor::main_reactor::MainEvent,
};

pub(super) fn initialize_component(
    effect_builder: EffectBuilder<MainEvent>,
    component: &mut impl InitializedComponent<MainEvent>,
    component_name: String,
    initiating_event: MainEvent,
) -> Option<Effects<MainEvent>> {
    if component.is_uninitialized() {
        let mut effects = effect_builder.immediately().event(|()| initiating_event);
        effects.extend(
            effect_builder
                .immediately()
                .event(|()| MainEvent::CheckStatus),
        );
        return Some(effects);
    }
    if component.is_fatal() {
        return Some(effect_builder.immediately().event(move |()| {
            MainEvent::Shutdown(format!("{} failed to initialize", component_name))
        }));
    }
    None
}
