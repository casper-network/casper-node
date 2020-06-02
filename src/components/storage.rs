mod block;
mod linear_block_store;

use std::{
    collections::HashSet,
    fmt::{self, Debug, Display, Formatter},
    sync::Arc,
};

use futures::FutureExt;
use smallvec::smallvec;
use tokio::task;
use tracing::info;

use crate::{
    components::Component,
    effect::{Effect, Multiple, Responder},
    types::Block,
};
pub(crate) use block::BlockType;
pub(crate) use linear_block_store::BlockStoreType;
use linear_block_store::InMemBlockStore;

pub(crate) type Storage = InMemStorage<Block>;

#[derive(Debug)]
pub(crate) enum Event<S: StorageType>
where
    <S::BlockStore as BlockStoreType>::Block: Debug,
{
    PutBlock {
        block: <S::BlockStore as BlockStoreType>::Block,
        responder: Responder<bool, Event<S>>,
    },
    GetBlock {
        block_hash: <<S::BlockStore as BlockStoreType>::Block as BlockType>::Hash,
        responder: Responder<Option<<S::BlockStore as BlockStoreType>::Block>, Event<S>>,
    },
}

impl<S: StorageType> Display for Event<S> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::PutBlock { block, .. } => write!(formatter, "put {}", block),
            Event::GetBlock { block_hash, .. } => write!(formatter, "get {}", block_hash),
        }
    }
}

// Trait which will handle management of the various storage sub-components.
//
// If this trait is ultimately only used for testing scenarios, we shouldn't need to expose it to
// the reactor - it can simply use a concrete type which implements this trait.
pub(crate) trait StorageType {
    type BlockStore: BlockStoreType + Send + Sync;

    fn block_store(&self) -> Arc<Self::BlockStore>;
}

impl<T> Component for T
where
    T: StorageType,
    Self: Sized + 'static,
{
    type Event = Event<Self>;

    fn handle_event(&mut self, event: Self::Event) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::PutBlock { block, responder } => {
                let block_store = self.block_store();
                let future = async move {
                    task::spawn_blocking(move || block_store.put(block))
                        .await
                        .expect("should run")
                };
                smallvec![future.then(|is_success| responder.call(is_success)).boxed()]
            }
            Event::GetBlock {
                block_hash,
                responder,
            } => {
                let block_store = self.block_store();
                let future = async move {
                    task::spawn_blocking(move || block_store.get(&block_hash))
                        .await
                        .expect("should run")
                };
                smallvec![future.then(|block| responder.call(block)).boxed()]
            }
        }
    }
}

// Concrete type of `Storage` - backed by in-memory block store only for now, but will eventually
// also hold in-mem versions of wasm-store, deploy-store, etc.
#[derive(Debug)]
pub(crate) struct InMemStorage<B: BlockType> {
    block_store: Arc<InMemBlockStore<B>>,
}

impl<B: BlockType> InMemStorage<B> {
    pub(crate) fn new() -> Self {
        InMemStorage {
            block_store: Arc::new(InMemBlockStore::new()),
        }
    }
}

impl<B: BlockType> StorageType for InMemStorage<B> {
    type BlockStore = InMemBlockStore<B>;

    fn block_store(&self) -> Arc<Self::BlockStore> {
        Arc::clone(&self.block_store)
    }
}

pub(crate) mod dummy {
    use std::time::Duration;

    use rand::{self, Rng};

    use super::*;
    use crate::{
        crypto::hash,
        effect::{EffectBuilder, EffectExt},
        reactor::Reactor,
        types::Block,
    };

    #[derive(Debug)]
    pub(crate) enum Event {
        Trigger,
        PutBlockSucceeded(<Block as BlockType>::Hash),
        PutBlockFailed(<Block as BlockType>::Hash),
        GotBlock(<Block as BlockType>::Hash, Option<Block>),
    }

    impl Display for Event {
        fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
            match self {
                Event::Trigger => write!(formatter, "Trigger"),
                Event::PutBlockSucceeded(block_hash) => {
                    write!(formatter, "put {} succeeded", block_hash)
                }
                Event::PutBlockFailed(block_hash) => write!(formatter, "put {} failed", block_hash),
                Event::GotBlock(block_hash, maybe_block) => {
                    if maybe_block.is_some() {
                        write!(formatter, "got block {}", block_hash)
                    } else {
                        write!(formatter, "failed to get block {}", block_hash)
                    }
                }
            }
        }
    }

    #[derive(Debug)]
    pub(crate) struct StorageConsumer {
        stored_blocks_hashes: HashSet<<Block as BlockType>::Hash>,
    }

    impl StorageConsumer {
        pub(crate) fn new<R: Reactor + 'static>(
            storage_effect_builder: EffectBuilder<R, super::Event<Storage>>,
        ) -> (Self, Multiple<Effect<Event>>) {
            (
                Self {
                    stored_blocks_hashes: HashSet::new(),
                },
                Self::set_timeout(storage_effect_builder),
            )
        }

        pub(crate) fn handle_event<R: Reactor + 'static>(
            &mut self,
            storage_effect_builder: EffectBuilder<R, super::Event<Storage>>,
            event: Event,
        ) -> Multiple<Effect<Event>> {
            match event {
                Event::Trigger => {
                    let mut rng = rand::thread_rng();
                    let create_block: bool = rng.gen();
                    if create_block {
                        let block = Block::new(rng.gen());
                        let block_hash = *block.hash();
                        self.stored_blocks_hashes.insert(block_hash);
                        Self::request_put_block(storage_effect_builder, block)
                    } else {
                        let block_hash = hash::hash(&[rng.gen::<u8>()]);
                        Self::request_get_block(storage_effect_builder, block_hash)
                    }
                }
                Event::PutBlockSucceeded(block_hash) => {
                    info!("consumer knows {} has been stored.", block_hash);
                    Self::set_timeout(storage_effect_builder)
                }
                Event::PutBlockFailed(block_hash) => {
                    info!("consumer knows {} has failed to be stored.", block_hash);
                    Self::set_timeout(storage_effect_builder)
                }
                Event::GotBlock(block_hash, maybe_block) => {
                    match &maybe_block {
                        Some(block) => info!("consumer got {}", block),
                        None => info!("consumer failed to get {}.", block_hash),
                    }
                    assert_eq!(
                        maybe_block.is_some(),
                        self.stored_blocks_hashes.contains(&block_hash)
                    );
                    Self::set_timeout(storage_effect_builder)
                }
            }
        }

        fn set_timeout<R: Reactor + 'static>(
            storage_effect_builder: EffectBuilder<R, super::Event<Storage>>,
        ) -> Multiple<Effect<Event>> {
            storage_effect_builder
                .set_timeout(Duration::from_millis(10))
                .event(|_| Event::Trigger)
        }

        fn request_put_block<R: Reactor + 'static>(
            storage_effect_builder: EffectBuilder<R, super::Event<Storage>>,
            block: Block,
        ) -> Multiple<Effect<Event>> {
            let block_hash = *block.hash();
            storage_effect_builder
                .make_request(|responder| super::Event::PutBlock { block, responder })
                .event(move |is_success| {
                    if is_success {
                        Event::PutBlockSucceeded(block_hash)
                    } else {
                        Event::PutBlockFailed(block_hash)
                    }
                })
        }

        fn request_get_block<R: Reactor + 'static>(
            storage_effect_builder: EffectBuilder<R, super::Event<Storage>>,
            block_hash: <Block as BlockType>::Hash,
        ) -> Multiple<Effect<Event>> {
            storage_effect_builder
                .make_request(move |responder| super::Event::GetBlock {
                    block_hash,
                    responder,
                })
                .event(move |maybe_block| Event::GotBlock(block_hash, maybe_block))
        }
    }
}
