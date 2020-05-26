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

use crate::effect::{Effect, Multiple, Responder};
pub(crate) use block::{BlockType, CLBlock};
pub(crate) use linear_block_store::BlockStoreType;
use linear_block_store::InMemBlockStore;

pub(crate) type Storage = InMemStorage<CLBlock>;

pub(crate) enum Event<S: StorageType>
where
    <S::BlockStore as BlockStoreType>::Block: Debug,
{
    PutBlock {
        block: <S::BlockStore as BlockStoreType>::Block,
        responder: Responder<bool, Event<S>>,
    },
    GetBlock {
        name: <<S::BlockStore as BlockStoreType>::Block as BlockType>::Name,
        responder: Responder<Option<<S::BlockStore as BlockStoreType>::Block>, Event<S>>,
    },
}

impl<S: StorageType> Debug for Event<S> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::PutBlock { block, .. } => {
                write!(formatter, "Event::PutBlock {{ block: {:?} }}", block)
            }
            Event::GetBlock { name, .. } => {
                write!(formatter, "Event::GetBlock {{ name: {:?} }}", name)
            }
        }
    }
}

impl<S: StorageType> Display for Event<S> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::PutBlock { block, .. } => write!(formatter, "put {}", block),
            Event::GetBlock { name, .. } => write!(formatter, "get {}", name),
        }
    }
}

// Trait which will handle management of the various storage sub-components.
//
// If this trait is ultimately only used for testing scenarios, we shouldn't need to expose it to
// the reactor - it can simply use a concrete type which implements this trait.
pub(crate) trait StorageType {
    type BlockStore: BlockStoreType + Send + Sync;

    fn handle_event(&mut self, event: Event<Self>) -> Multiple<Effect<Event<Self>>>
    where
        Self: Sized + 'static,
    {
        match event {
            Event::PutBlock { block, responder } => {
                let block_store = self.block_store();
                let future = async move {
                    task::spawn_blocking(move || block_store.put(block))
                        .await
                        .expect("should run")
                };
                smallvec![future.then(|is_success| responder(is_success)).boxed()]
            }
            Event::GetBlock { name, responder } => {
                let block_store = self.block_store();
                let future = async move {
                    task::spawn_blocking(move || block_store.get(&name))
                        .await
                        .expect("should run")
                };
                smallvec![future.then(|block| responder(block)).boxed()]
            }
        }
    }

    fn block_store(&self) -> Arc<Self::BlockStore>;
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
        effect::{EffectBuilder, EffectExt},
        reactor::Reactor,
    };

    #[derive(Debug)]
    pub(crate) enum Event {
        Trigger,
        PutBlockSucceeded(u8),
        PutBlockFailed(u8),
        GetBlock(u8, Option<CLBlock>),
    }

    impl Display for Event {
        fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
            match self {
                Event::Trigger => write!(formatter, "Trigger"),
                Event::PutBlockSucceeded(name) => write!(formatter, "PutBlockSucceeded({})", name),
                Event::PutBlockFailed(name) => write!(formatter, "PutBlockFailed({})", name),
                Event::GetBlock(name, maybe_block) => write!(
                    formatter,
                    "GetBlock {{ {}, {} }}",
                    name,
                    maybe_block.is_some()
                ),
            }
        }
    }

    #[derive(Debug)]
    pub(crate) struct StorageConsumer {
        stored_blocks_names: HashSet<u8>,
    }

    impl StorageConsumer {
        pub(crate) fn new<R: Reactor + 'static>(
            storage_effect_builder: EffectBuilder<R, super::Event<Storage>>,
        ) -> (Self, Multiple<Effect<Event>>) {
            (
                Self {
                    stored_blocks_names: HashSet::new(),
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
                        let block = CLBlock::new(rng.gen(), rng.gen());
                        let name = *block.name();
                        self.stored_blocks_names.insert(name);
                        Self::request_put_block(storage_effect_builder, block)
                    } else {
                        let name = rng.gen();
                        Self::request_get_block(storage_effect_builder, name)
                    }
                }
                Event::PutBlockSucceeded(name) => {
                    info!("Consumer knows {} has been stored.", name);
                    Self::set_timeout(storage_effect_builder)
                }
                Event::PutBlockFailed(name) => {
                    info!("Consumer knows {} has failed to be stored.", name);
                    Self::set_timeout(storage_effect_builder)
                }
                Event::GetBlock(name, maybe_block) => {
                    info!("Consumer received {:?}.", maybe_block);
                    assert_eq!(
                        maybe_block.is_some(),
                        self.stored_blocks_names.contains(&name)
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
            block: CLBlock,
        ) -> Multiple<Effect<Event>> {
            let name = *block.name();
            storage_effect_builder
                .make_request(|responder| super::Event::PutBlock { block, responder })
                .event(move |is_success| {
                    if is_success {
                        Event::PutBlockSucceeded(name)
                    } else {
                        Event::PutBlockFailed(name)
                    }
                })
        }

        fn request_get_block<R: Reactor + 'static>(
            storage_effect_builder: EffectBuilder<R, super::Event<Storage>>,
            name: u8,
        ) -> Multiple<Effect<Event>> {
            storage_effect_builder
                .make_request(move |responder| super::Event::GetBlock { name, responder })
                .event(move |maybe_block| Event::GetBlock(name, maybe_block))
        }
    }
}
