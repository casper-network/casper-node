use crate::{
    components::InitializedComponent,
    effect::{EffectBuilder, EffectExt, Effects},
    reactor::main_reactor::MainEvent,
};

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
