#![allow(clippy::boxed_local)] // We use boxed locals to pass on event data unchanged.

//! The `FakeTransactionAcceptor` behaves as per the real `TransactionAcceptor` but without any
//! transaction verification being performed.
//!
//! When a new transaction is passed in, it is unconditionally accepted.  This means that the
//! `FakeTransactionAcceptor` puts the transaction to storage, and once that has completed,
//! announces the transaction if the storage result indicates it's a new transaction.

use std::sync::Arc;

use tracing::debug;

use casper_types::Transaction;

pub(crate) use crate::components::transaction_acceptor::{Error, Event};
use crate::{
    components::{transaction_acceptor::EventMetadata, Component},
    effect::{
        announcements::TransactionAcceptorAnnouncement, requests::StorageRequest, EffectBuilder,
        EffectExt, Effects, Responder,
    },
    utils::Source,
    NodeRng,
};

const COMPONENT_NAME: &str = "fake_transaction_acceptor";

pub(crate) trait ReactorEventT:
    From<Event> + From<TransactionAcceptorAnnouncement> + From<StorageRequest> + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event> + From<TransactionAcceptorAnnouncement> + From<StorageRequest> + Send
{
}

#[derive(Debug)]
pub struct FakeTransactionAcceptor {
    is_active: bool,
}

impl FakeTransactionAcceptor {
    pub(crate) fn new() -> Self {
        FakeTransactionAcceptor { is_active: true }
    }

    pub(crate) fn set_active(&mut self, new_setting: bool) {
        self.is_active = new_setting;
    }

    fn accept<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        transaction: Transaction,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let event_metadata = Box::new(EventMetadata::new(
            transaction.clone(),
            source,
            maybe_responder,
        ));
        effect_builder
            .put_transaction_to_storage(transaction)
            .event(move |is_new| Event::PutToStorageResult {
                event_metadata,
                is_new,
            })
    }

    fn handle_put_to_storage<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        is_new: bool,
    ) -> Effects<Event> {
        let EventMetadata {
            transaction,
            source,
            maybe_responder,
            verification_start_timestamp: _,
        } = *event_metadata;
        let mut effects = Effects::new();
        if is_new {
            effects.extend(
                effect_builder
                    .announce_new_transaction_accepted(Arc::new(transaction), source)
                    .ignore(),
            );
        }

        if let Some(responder) = maybe_responder {
            effects.extend(responder.respond(Ok(())).ignore());
        }
        effects
    }
}

impl<REv: ReactorEventT> Component<REv> for FakeTransactionAcceptor {
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        if !self.is_active {
            debug!(
                ?event,
                "FakeTransactionAcceptor: not active - ignoring event"
            );
            return Effects::new();
        }
        debug!(?event, "FakeTransactionAcceptor: handling event");
        match event {
            Event::Accept {
                transaction,
                source,
                maybe_responder,
            } => self.accept(effect_builder, transaction, source, maybe_responder),
            Event::PutToStorageResult {
                event_metadata,
                is_new,
                ..
            } => self.handle_put_to_storage(effect_builder, event_metadata, is_new),
            _ => unimplemented!("unexpected {:?}", event),
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}
