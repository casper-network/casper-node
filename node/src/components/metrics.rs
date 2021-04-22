//! Metrics component.
//!
//! The metrics component renders metrics upon request.
//!
//! # Adding metrics to a component
//!
//! When adding metrics to an existing component, there are a few guidelines that should in general
//! be followed:
//!
//! 1. For a component `XYZ`, there should be a `XYZMetrics` struct that is one of its fields that
//!    holds all of the `Collectors` (`Counter`s, etc) to make it easy to find all of the metrics
//!    for a component in one place.
//!
//!    Creation and instantiation of this component happens inside the `reactor::Reactor::new`
//!    function, which is passed in a `prometheus::Registry` (see 2.).
//!
//! 2. Instantiation of an `XYZMetrics` struct should always be combined with registering all of
//!    the metrics on a registry. For this reason it is advisable to have the `XYZMetrics::new`
//!    method take a `prometheus::Registry` and register it directly.
//!
//! 3. Updating metrics is done inside the `handle_event` function by simply calling methods on the
//!    fields of `self.metrics` (`: XYZMetrics`). **Important**: Metrics should never be read to
//!    prevent any actual logic depending on them. If a counter is being increment as a metric and
//!    also required for business logic, a second counter should be kept in the component's state.

use std::convert::Infallible;

use datasize::DataSize;
use prometheus::{Encoder, Registry, TextEncoder};
use tracing::error;

use crate::{
    components::Component,
    effect::{requests::MetricsRequest, EffectBuilder, EffectExt, Effects},
    NodeRng,
};

/// The metrics component.
#[derive(DataSize, Debug)]
pub(crate) struct Metrics {
    /// Metrics registry used to answer metrics queries.
    #[data_size(skip)] // Actual implementation is just a wrapper around an `Arc`.
    registry: Registry,
}

impl<REv> Component<REv> for Metrics {
    type Event = MetricsRequest;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        req: Self::Event,
    ) -> Effects<Self::Event> {
        match req {
            MetricsRequest::RenderNodeMetricsText { responder } => {
                let mut buf: Vec<u8> = Vec::<u8>::new();

                if let Err(e) = TextEncoder::new().encode(&self.registry.gather(), &mut buf) {
                    error!(%e, "text encoding of metrics failed");
                    return responder.respond(None).ignore();
                };

                match String::from_utf8(buf) {
                    Ok(text) => responder.respond(Some(text)).ignore(),
                    Err(e) => {
                        error!(%e, "generated text metrics are not valid UTF-8");
                        responder.respond(None).ignore()
                    }
                }
            }
        }
    }
}

impl Metrics {
    /// Create and initialize a new metrics component.
    pub(crate) fn new(registry: Registry) -> Self {
        Metrics { registry }
    }
}
