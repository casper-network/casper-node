mod block;
mod linear_block_store;

use std::{
    collections::HashSet,
    fmt::{self, Debug, Display, Formatter},
    sync::Arc,
};

use tokio::task;
use tracing::info;

use crate::{
    components::Component,
    effect::{requests::StorageRequest, Effect, EffectBuilder, EffectExt, Multiple},
    types::Block,
};
pub(crate) use block::BlockType;
pub(crate) use linear_block_store::BlockStoreType;
use linear_block_store::InMemBlockStore;

pub(crate) type Storage = InMemStorage<Block>;

// Trait which will handle management of the various storage sub-components.
//
// If this trait is ultimately only used for testing scenarios, we shouldn't need to expose it to
// the reactor - it can simply use a concrete type which implements this trait.
pub(crate) trait StorageType {
    type BlockStore: BlockStoreType + Send + Sync;

    fn block_store(&self) -> Arc<Self::BlockStore>;
}

impl<REv, S> Component<REv> for S
where
    S: StorageType,
    Self: Sized + 'static,
{
    type Event = StorageRequest<Self>;

    fn handle_event(
        &mut self,
        _eb: EffectBuilder<REv>,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            StorageRequest::PutBlock { block, responder } => {
                let block_store = self.block_store();
                async move {
                    let result = task::spawn_blocking(move || block_store.put(block))
                        .await
                        .expect("should run");
                    responder.respond(result).await
                }
                .ignore()
            }
            StorageRequest::GetBlock {
                block_hash,
                responder,
            } => {
                let block_store = self.block_store();
                async move {
                    let result = task::spawn_blocking(move || block_store.get(&block_hash))
                        .await
                        .expect("should run");
                    responder.respond(result).await
                }
                .ignore()
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
        effect::{requests::StorageRequest, EffectBuilder, EffectExt},
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

    impl<REv> Component<REv> for StorageConsumer
    where
        REv: From<Event> + Send + From<StorageRequest<Storage>>,
    {
        type Event = Event;

        fn handle_event(
            &mut self,
            eb: EffectBuilder<REv>,
            event: Self::Event,
        ) -> Multiple<Effect<Self::Event>> {
            match event {
                Event::Trigger => {
                    let mut rng = rand::thread_rng();
                    let create_block: bool = rng.gen();
                    if create_block {
                        let block = Block::new(rng.gen());
                        let block_hash = *block.hash();
                        self.stored_blocks_hashes.insert(block_hash);
                        Self::request_put_block(eb, block)
                    } else {
                        let block_hash = hash::hash(&[rng.gen::<u8>()]);
                        Self::request_get_block(eb, block_hash)
                    }
                }
                Event::PutBlockSucceeded(block_hash) => {
                    info!("consumer knows {} has been stored.", block_hash);
                    Self::set_timeout(eb)
                }
                Event::PutBlockFailed(block_hash) => {
                    info!("consumer knows {} has failed to be stored.", block_hash);
                    Self::set_timeout(eb)
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
                    Self::set_timeout(eb)
                }
            }
        }
    }

    impl StorageConsumer {
        pub(crate) fn new<REv>(eb: EffectBuilder<REv>) -> (Self, Multiple<Effect<Event>>)
        where
            REv: From<Event> + Send + From<StorageRequest<Storage>>,
        {
            (
                Self {
                    stored_blocks_hashes: HashSet::new(),
                },
                Self::set_timeout(eb),
            )
        }

        fn set_timeout<REv>(eb: EffectBuilder<REv>) -> Multiple<Effect<Event>>
        where
            REv: From<Event> + Send + From<StorageRequest<Storage>>,
        {
            eb.set_timeout(Duration::from_millis(10))
                .event(|_| Event::Trigger)
        }

        fn request_put_block<REv>(eb: EffectBuilder<REv>, block: Block) -> Multiple<Effect<Event>>
        where
            REv: From<Event> + Send + From<StorageRequest<Storage>>,
        {
            let block_hash = *block.hash();
            eb.put_block(block).event(move |is_success| {
                if is_success {
                    Event::PutBlockSucceeded(block_hash)
                } else {
                    Event::PutBlockFailed(block_hash)
                }
            })
        }

        fn request_get_block<REv>(
            eb: EffectBuilder<REv>,
            block_hash: <Block as BlockType>::Hash,
        ) -> Multiple<Effect<Event>>
        where
            REv: From<Event> + Send + From<StorageRequest<Storage>>,
        {
            eb.get_block(block_hash)
                .event(move |maybe_block| Event::GotBlock(block_hash, maybe_block))
        }
    }
}
