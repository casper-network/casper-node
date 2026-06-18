#![allow(clippy::boxed_local)] // We use boxed locals to pass on event data unchanged.

//! The `FakeTransactionAcceptor` behaves as per the real `TransactionAcceptor` but without any
//! transaction verification being performed.
//!
//! When a new transaction is passed in, it is unconditionally accepted.  This means that the
//! `FakeTransactionAcceptor` puts the transaction to storage, and once that has completed,
//! announces the transaction if the storage result indicates it's a new transaction.

use std::sync::Arc;

use tracing::{debug, trace};

use casper_types::{Block, BlockHeader, Chainspec, Timestamp, Transaction};

pub(crate) use crate::components::transaction_acceptor::{Error, Event};
use crate::{
    components::{transaction_acceptor::EventMetadata, Component},
    effect::{
        announcements::TransactionAcceptorAnnouncement, requests::StorageRequest, EffectBuilder,
        EffectExt, Effects, Responder,
    },
    types::MetaTransaction,
    utils::Source,
    NodeRng,
};
use crate::types::TransactionProvenance;

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
    chainspec: Chainspec,
}

impl FakeTransactionAcceptor {
    pub(crate) fn new() -> Self {
        FakeTransactionAcceptor {
            is_active: true,
            chainspec: Chainspec::default(),
        }
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
        let meta_transaction = MetaTransaction::from_transaction(
            &transaction,
            self.chainspec.core_config.pricing_handling,
            &self.chainspec.transaction_config,
        )
        .unwrap();
        let provenance = match source {
            Source::PeerGossiped(_)  | Source::Peer(_) => {
                TransactionProvenance::Gossiped
            }
            Source::Client | Source::SpeculativeExec | Source::Ourself=> {
                TransactionProvenance::Client
            }
        };
        let event_metadata = Box::new(EventMetadata::new(
            transaction.clone(),
            meta_transaction,
            source,
            maybe_responder,
            Timestamp::now(),
            provenance,
            None,
        ));

        let fake_block = Arc::new(Block::example().clone());

        effect_builder
            .put_block_to_storage(Arc::clone(&fake_block))
            .event(move |_| Event::GetBlockHeaderResult {
                event_metadata,
                maybe_block_header: Some(Box::new(fake_block.clone_header())),
            })
    }

    fn handle_get_block_header<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        mut event_metadata: Box<EventMetadata>,
        maybe_block_header: Option<Box<BlockHeader>>,
    ) -> Effects<Event> {
        event_metadata.maybe_block_hash =
            Some(maybe_block_header.expect("must have header").block_hash());

        effect_builder
            .put_transaction_to_storage(event_metadata.transaction.clone())
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
            meta_transaction: _,
            transaction,
            source,
            maybe_responder,
            maybe_block_hash,
            provenance,
            ..
        } = *event_metadata;
        let mut effects = Effects::new();
        let block_hash = maybe_block_hash.expect("must have set block hash correctly");
        if is_new {
            effects.extend(
                effect_builder
                    .announce_new_transaction_accepted(
                        Arc::new(transaction),
                        source,
                        provenance,
                        block_hash,
                    )
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
        trace!(?event, "FakeTransactionAcceptor: handling event");
        match event {
            Event::Accept {
                transaction,
                source,
                maybe_responder,
                provenance: _,
                maybe_block_hash: _,
            } => self.accept(effect_builder, transaction, source, maybe_responder),
            Event::GetBlockHeaderResult {
                event_metadata,
                maybe_block_header,
            } => self.handle_get_block_header(effect_builder, event_metadata, maybe_block_header),
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
