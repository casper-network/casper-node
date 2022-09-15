use crate::{
    components::InitializedComponent,
    effect::{EffectBuilder, EffectExt, Effects},
    reactor::participating::ParticipatingEvent,
};

pub(super) fn initialize_component(
    effect_builder: EffectBuilder<ParticipatingEvent>,
    component: &mut impl InitializedComponent<ParticipatingEvent>,
    component_name: String,
    initiating_event: ParticipatingEvent,
) -> Option<Effects<ParticipatingEvent>> {
    if component.is_uninitialized() {
        let mut effects = Effects::new();
        effects.extend(effect_builder.immediately().event(|()| initiating_event));
        effects.extend(
            effect_builder
                .immediately()
                .event(|()| ParticipatingEvent::CheckStatus),
        );
        return Some(effects);
    }
    if component.is_fatal() {
        return Some(effect_builder.immediately().event(move |()| {
            ParticipatingEvent::Shutdown(format!("{} failed to initialize", component_name.clone()))
        }));
    }
    None
}
