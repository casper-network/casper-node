use futures::FutureExt;
use smallvec::smallvec;

use crate::{components::InitializedComponent, effect::Effects, reactor::main_reactor::MainEvent};

pub(super) fn new_shutdown_effect<T: ToString + Send + 'static>(message: T) -> Effects<MainEvent> {
    smallvec![async move { smallvec![MainEvent::Shutdown(message.to_string())] }.boxed()]
}

pub(super) fn initialize_component(
    component: &mut impl InitializedComponent<MainEvent>,
    component_name: &str,
    initiating_event: MainEvent,
) -> Option<Effects<MainEvent>> {
    if component.is_uninitialized() {
        return Some(smallvec![async { smallvec![initiating_event] }.boxed()]);
    }
    if component.is_fatal() {
        return Some(new_shutdown_effect(format!(
            "{} failed to initialize",
            component_name
        )));
    }
    None
}
