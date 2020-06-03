mod store;

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
    types::{Block, BlockHash, Deploy, DeployHash},
};
use store::InMemStore;
pub(crate) use store::{Store, Value};

pub(crate) type Storage = InMemStorage<Block, Deploy>;

// Trait which will handle management of the various storage sub-components.
//
// If this trait is ultimately only used for testing scenarios, we shouldn't need to expose it to
// the reactor - it can simply use a concrete type which implements this trait.
pub(crate) trait StorageType {
    type BlockStore: Store + Send + Sync;
    type DeployStore: Store + Send + Sync;

    fn block_store(&self) -> Arc<Self::BlockStore>;
    fn deploy_store(&self) -> Arc<Self::DeployStore>;
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
            StorageRequest::GetBlockHeader {
                block_hash,
                responder,
            } => {
                let block_store = self.block_store();
                async move {
                    let result = task::spawn_blocking(move || block_store.get_header(&block_hash))
                        .await
                        .expect("should run");
                    responder.respond(result).await
                }
                .ignore()
            }
            StorageRequest::PutDeploy { deploy, responder } => {
                let deploy_store = self.deploy_store();
                async move {
                    let result = task::spawn_blocking(move || deploy_store.put(deploy))
                        .await
                        .expect("should run");
                    responder.respond(result).await
                }
                .ignore()
            }
            StorageRequest::GetDeploy {
                deploy_hash,
                responder,
            } => {
                let deploy_store = self.deploy_store();
                async move {
                    let result = task::spawn_blocking(move || deploy_store.get(&deploy_hash))
                        .await
                        .expect("should run");
                    responder.respond(result).await
                }
                .ignore()
            }
            StorageRequest::GetDeployHeader {
                deploy_hash,
                responder,
            } => {
                let deploy_store = self.deploy_store();
                async move {
                    let result =
                        task::spawn_blocking(move || deploy_store.get_header(&deploy_hash))
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
pub(crate) struct InMemStorage<B: Value, D: Value> {
    block_store: Arc<InMemStore<B>>,
    deploy_store: Arc<InMemStore<D>>,
}

impl<B: Value, D: Value> InMemStorage<B, D> {
    pub(crate) fn new() -> Self {
        InMemStorage {
            block_store: Arc::new(InMemStore::new()),
            deploy_store: Arc::new(InMemStore::new()),
        }
    }
}

impl<B: Value, D: Value> StorageType for InMemStorage<B, D> {
    type BlockStore = InMemStore<B>;
    type DeployStore = InMemStore<D>;

    fn block_store(&self) -> Arc<Self::BlockStore> {
        Arc::clone(&self.block_store)
    }

    fn deploy_store(&self) -> Arc<Self::DeployStore> {
        Arc::clone(&self.deploy_store)
    }
}

pub(crate) mod dummy {
    use std::time::Duration;

    use rand::{self, Rng};

    use super::{store::InMemError, *};
    use crate::{
        crypto::hash,
        effect::{
            requests::StorageRequest, EffectBuilder, EffectExt, EffectOptionExt, EffectResultExt,
        },
    };

    #[derive(Debug)]
    #[allow(clippy::large_enum_variant)]
    pub(crate) enum Event {
        Trigger,
        PutBlockSucceeded(<Block as Value>::Id),
        PutBlockFailed(<Block as Value>::Id),
        GotBlock(<Block as Value>::Id, Block),
        GetBlockFailed(<Block as Value>::Id, InMemError),
        GotBlockHeader(<Block as Value>::Id, Option<<Block as Value>::Header>),
        PutDeploySucceeded(<Deploy as Value>::Id),
        PutDeployFailed(<Deploy as Value>::Id),
        GotDeploy(<Deploy as Value>::Id, Deploy),
        GetDeployFailed(<Deploy as Value>::Id, InMemError),
        GotDeployHeader(<Deploy as Value>::Id, <Deploy as Value>::Header),
        GetDeployHeaderFailed(<Deploy as Value>::Id),
    }

    impl Display for Event {
        fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
            match self {
                Event::Trigger => write!(formatter, "Trigger"),
                Event::PutBlockSucceeded(block_hash) => {
                    write!(formatter, "put {} succeeded", block_hash)
                }
                Event::PutBlockFailed(block_hash) => write!(formatter, "put {} failed", block_hash),
                Event::GotBlock(block_hash, _block) => {
                    write!(formatter, "got block {}", block_hash)
                }
                Event::GetBlockFailed(block_hash, error) => {
                    write!(formatter, "failed to get block {}: {}", block_hash, error)
                }
                Event::GotBlockHeader(block_hash, maybe_block_header) => {
                    if maybe_block_header.is_some() {
                        write!(formatter, "got block header {}", block_hash)
                    } else {
                        write!(formatter, "failed to get block header {}", block_hash)
                    }
                }
                Event::PutDeploySucceeded(deploy_hash) => {
                    write!(formatter, "put {} succeeded", deploy_hash)
                }
                Event::PutDeployFailed(deploy_hash) => {
                    write!(formatter, "put {} failed", deploy_hash)
                }
                Event::GotDeploy(deploy_hash, _deploy) => {
                    write!(formatter, "got deploy {}", deploy_hash)
                }
                Event::GetDeployFailed(deploy_hash, error) => {
                    write!(formatter, "failed to get deploy {}: {}", deploy_hash, error)
                }
                Event::GotDeployHeader(deploy_hash, _deploy_header) => {
                    write!(formatter, "got deploy header {}", deploy_hash)
                }
                Event::GetDeployHeaderFailed(deploy_hash) => {
                    write!(formatter, "failed to get deploy header {}", deploy_hash)
                }
            }
        }
    }

    #[derive(Debug)]
    pub(crate) struct StorageConsumer {
        stored_blocks_hashes: HashSet<<Block as Value>::Id>,
        stored_deploys_hashes: HashSet<<Deploy as Value>::Id>,
    }

    impl<REv> Component<REv> for StorageConsumer
    where
        REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
    {
        type Event = Event;

        #[allow(clippy::cognitive_complexity)]
        fn handle_event(
            &mut self,
            eb: EffectBuilder<REv>,
            event: Self::Event,
        ) -> Multiple<Effect<Self::Event>> {
            match event {
                Event::Trigger => {
                    let mut rng = rand::thread_rng();
                    let create_block_and_deploy: bool = rng.gen();
                    let mut effects;
                    if create_block_and_deploy {
                        let block = Block::new(rng.gen());
                        let block_hash = *block.id();
                        self.stored_blocks_hashes.insert(block_hash);
                        effects = Self::request_put_block(eb, block);

                        let deploy = Deploy::new(rng.gen());
                        let deploy_hash = *deploy.id();
                        self.stored_deploys_hashes.insert(deploy_hash);
                        effects.extend(Self::request_put_deploy(eb, deploy));
                    } else {
                        let block_hash = BlockHash::new(hash::hash(&[rng.gen::<u8>()]));
                        effects = Self::request_get_block(eb, block_hash);
                        effects.extend(Self::request_get_block_header(eb, block_hash));

                        let deploy_hash = DeployHash::new(hash::hash(&[rng.gen::<u8>()]));
                        effects.extend(Self::request_get_deploy(eb, deploy_hash));
                        effects.extend(Self::request_get_deploy_header(eb, deploy_hash));
                    }
                    effects
                }
                Event::PutBlockSucceeded(block_hash) => {
                    info!("consumer knows {} has been stored.", block_hash);
                    Self::set_timeout(eb)
                }
                Event::PutBlockFailed(block_hash) => {
                    info!("consumer knows {} has failed to be stored.", block_hash);
                    Self::set_timeout(eb)
                }
                Event::GotBlock(block_hash, block) => {
                    info!("consumer got {}", block);
                    assert!(self.stored_blocks_hashes.contains(&block_hash));
                    Self::set_timeout(eb)
                }
                Event::GetBlockFailed(block_hash, _error) => {
                    info!("consumer failed to get block {}.", block_hash);
                    assert!(!self.stored_blocks_hashes.contains(&block_hash));
                    Self::set_timeout(eb)
                }
                Event::GotBlockHeader(block_hash, maybe_block_header) => {
                    match &maybe_block_header {
                        Some(block_header) => info!("consumer got {}", block_header),
                        None => info!("consumer failed to get block header {}.", block_hash),
                    }
                    assert_eq!(
                        maybe_block_header.is_some(),
                        self.stored_blocks_hashes.contains(&block_hash)
                    );
                    eb.do_nothing().ignore()
                }
                Event::PutDeploySucceeded(deploy_hash) => {
                    info!("consumer knows {} has been stored.", deploy_hash);
                    eb.do_nothing().ignore()
                }
                Event::PutDeployFailed(deploy_hash) => {
                    info!("consumer knows {} has failed to be stored.", deploy_hash);
                    eb.do_nothing().ignore()
                }
                Event::GotDeploy(deploy_hash, deploy) => {
                    info!("consumer got {}", deploy);
                    assert!(self.stored_deploys_hashes.contains(&deploy_hash));
                    eb.do_nothing().ignore()
                }
                Event::GetDeployFailed(deploy_hash, _error) => {
                    info!("consumer failed to get deploy {}.", deploy_hash);
                    assert!(!self.stored_deploys_hashes.contains(&deploy_hash));
                    eb.do_nothing().ignore()
                }
                Event::GotDeployHeader(deploy_hash, deploy_header) => {
                    info!("consumer got {}", deploy_header);
                    assert!(self.stored_deploys_hashes.contains(&deploy_hash));
                    eb.do_nothing().ignore()
                }
                Event::GetDeployHeaderFailed(deploy_hash) => {
                    info!("consumer failed to get deploy header {}.", deploy_hash);
                    assert!(!self.stored_deploys_hashes.contains(&deploy_hash));
                    eb.do_nothing().ignore()
                }
            }
        }
    }

    impl StorageConsumer {
        pub(crate) fn new<REv>(eb: EffectBuilder<REv>) -> (Self, Multiple<Effect<Event>>)
        where
            REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
        {
            (
                Self {
                    stored_blocks_hashes: HashSet::new(),
                    stored_deploys_hashes: HashSet::new(),
                },
                Self::set_timeout(eb),
            )
        }

        fn set_timeout<REv>(eb: EffectBuilder<REv>) -> Multiple<Effect<Event>>
        where
            REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
        {
            eb.set_timeout(Duration::from_millis(10))
                .event(|_| Event::Trigger)
        }

        fn request_put_block<REv>(eb: EffectBuilder<REv>, block: Block) -> Multiple<Effect<Event>>
        where
            REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
        {
            let block_hash = *block.id();
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
            block_hash: <Block as Value>::Id,
        ) -> Multiple<Effect<Event>>
        where
            REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
        {
            eb.get_block(block_hash).result(
                move |block| Event::GotBlock(block_hash, block),
                move |error| Event::GetBlockFailed(block_hash, error),
            )
        }

        fn request_get_block_header<REv>(
            eb: EffectBuilder<REv>,
            block_hash: <Block as Value>::Id,
        ) -> Multiple<Effect<Event>>
        where
            REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
        {
            eb.get_block_header(block_hash)
                .event(move |maybe_block_header| {
                    Event::GotBlockHeader(block_hash, maybe_block_header)
                })
        }

        fn request_put_deploy<REv>(
            eb: EffectBuilder<REv>,
            deploy: Deploy,
        ) -> Multiple<Effect<Event>>
        where
            REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
        {
            let deploy_hash = *deploy.id();
            eb.put_deploy(deploy).event(move |is_success| {
                if is_success {
                    Event::PutDeploySucceeded(deploy_hash)
                } else {
                    Event::PutDeployFailed(deploy_hash)
                }
            })
        }

        fn request_get_deploy<REv>(
            eb: EffectBuilder<REv>,
            deploy_hash: <Deploy as Value>::Id,
        ) -> Multiple<Effect<Event>>
        where
            REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
        {
            eb.get_deploy(deploy_hash).result(
                move |deploy| Event::GotDeploy(deploy_hash, deploy),
                move |error| Event::GetDeployFailed(deploy_hash, error),
            )
        }

        fn request_get_deploy_header<REv>(
            eb: EffectBuilder<REv>,
            deploy_hash: <Deploy as Value>::Id,
        ) -> Multiple<Effect<Event>>
        where
            REv: From<Box<Event>> + Send + From<Box<StorageRequest<Storage>>>,
        {
            eb.get_deploy_header(deploy_hash).option(
                move |deploy_header| Event::GotDeployHeader(deploy_hash, deploy_header),
                move || Event::GetDeployHeaderFailed(deploy_hash),
            )
        }
    }
}
