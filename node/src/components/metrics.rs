//! Metrics component.
//!
//! The metrics component renders metrics upon request.

use prometheus::{Encoder, Registry, TextEncoder};
use rand::Rng;
use tracing::error;

use crate::{
    components::Component,
    effect::{requests::MetricsRequest, EffectBuilder, EffectExt, Effects},
};

/// The metrics component.
#[derive(Debug)]
pub(crate) struct Metrics {
    /// Metrics registry used to answer metrics queries.
    registry: Registry,
}

impl<REv> Component<REv> for Metrics {
    type Event = MetricsRequest;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
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
