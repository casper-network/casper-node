mod error;
mod in_mem_store;
mod store;

use std::{
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
    sync::Arc,
};

use rand::Rng;
use serde::{de::DeserializeOwned, Serialize};
use tokio::task;
use tracing::info;

use crate::{
    components::Component,
    effect::{requests::StorageRequest, Effect, EffectBuilder, EffectExt, Multiple},
    types::{Block, Deploy},
};
pub(crate) use error::{InMemError, InMemResult};
use in_mem_store::InMemStore;
pub(crate) use store::Store;

pub(crate) type Storage = InMemStorage<Block, Deploy>;

/// Trait defining the API for a value able to be held within the storage component.
pub(crate) trait Value:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display
{
    type Id: Copy + Clone + Ord + PartialOrd + Eq + PartialEq + Hash + Debug + Display + Send + Sync;
    /// A relatively small portion of the value, representing header info or metadata.
    type Header: Clone + Ord + PartialOrd + Eq + PartialEq + Hash + Debug + Display + Send + Sync;

    fn id(&self) -> &Self::Id;
    fn header(&self) -> &Self::Header;
}

/// Trait which will handle management of the various storage sub-components.
///
/// If this trait is ultimately only used for testing scenarios, we shouldn't need to expose it to
/// the reactor - it can simply use a concrete type which implements this trait.
pub(crate) trait StorageType {
    type BlockStore: Store + Send + Sync;
    type DeployStore: Store + Send + Sync;
    type Error: StdError
        + StdError
        + Clone
        + Serialize
        + DeserializeOwned
        + Send
        + Sync
        + From<<Self::BlockStore as Store>::Error>
        + From<<Self::DeployStore as Store>::Error>;

    fn block_store(&self) -> Arc<Self::BlockStore>;
    fn deploy_store(&self) -> Arc<Self::DeployStore>;
}

impl<REv, S> Component<REv> for S
where
    S: StorageType,
    Self: Sized + 'static,
{
    type Event = StorageRequest<Self>;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        _eb: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            StorageRequest::PutBlock { block, responder } => {
                let block_store = self.block_store();
                async move {
                    let result = task::spawn_blocking(move || block_store.put(*block))
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
                    let result = task::spawn_blocking(move || deploy_store.put(*deploy))
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
    type Error = InMemError;

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

    use super::*;
    use crate::{
        crypto::hash,
        effect::{requests::StorageRequest, EffectBuilder, EffectExt},
        types::{Block, BlockHash, BlockHeader, Deploy, DeployHash, DeployHeader},
    };

    #[derive(Debug)]
    #[allow(clippy::large_enum_variant)]
    pub(crate) enum Event {
        Trigger,
        PutBlockResult {
            block_hash: BlockHash,
            result: InMemResult<()>,
        },
        GetBlockResult {
            block_hash: BlockHash,
            result: InMemResult<Block>,
        },
        GetBlockHeaderResult {
            block_hash: BlockHash,
            result: InMemResult<BlockHeader>,
        },
        PutDeployResult {
            deploy_hash: DeployHash,
            result: InMemResult<()>,
        },
        GetDeployResult {
            deploy_hash: DeployHash,
            result: InMemResult<Deploy>,
        },
        GetDeployHeaderResult {
            deploy_hash: DeployHash,
            result: InMemResult<DeployHeader>,
        },
    }

    impl Display for Event {
        fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
            match self {
                Event::Trigger => write!(formatter, "Trigger"),
                Event::PutBlockResult { block_hash, result } => {
                    write!(formatter, "PutBlockResult for {}: {:?}", block_hash, result)
                }
                Event::GetBlockResult { block_hash, result } => {
                    write!(formatter, "GetBlockResult for {}: {:?}", block_hash, result)
                }
                Event::GetBlockHeaderResult { block_hash, result } => write!(
                    formatter,
                    "GetBlockHeaderResult for {}: {:?}",
                    block_hash, result
                ),
                Event::PutDeployResult {
                    deploy_hash,
                    result,
                } => write!(
                    formatter,
                    "PutDeployResult for {}: {:?}",
                    deploy_hash, result
                ),
                Event::GetDeployResult {
                    deploy_hash,
                    result,
                } => write!(
                    formatter,
                    "GetDeployResult for {}: {:?}",
                    deploy_hash, result
                ),
                Event::GetDeployHeaderResult {
                    deploy_hash,
                    result,
                } => write!(
                    formatter,
                    "GetDeployHeaderResult for {}: {:?}",
                    deploy_hash, result
                ),
            }
        }
    }

    #[derive(Debug)]
    pub(crate) struct StorageConsumer {}

    impl<REv> Component<REv> for StorageConsumer
    where
        REv: From<Box<Event>> + Send + From<StorageRequest<Storage>>,
    {
        type Event = Event;

        #[allow(clippy::cognitive_complexity)]
        fn handle_event<R: Rng + ?Sized>(
            &mut self,
            eb: EffectBuilder<REv>,
            rng: &mut R,
            event: Self::Event,
        ) -> Multiple<Effect<Self::Event>> {
            match event {
                Event::Trigger => {
                    let create_block_and_deploy: bool = rng.gen();
                    let mut effects;
                    if create_block_and_deploy {
                        let block = Block::new(rng.gen());
                        let block_hash = *block.id();
                        effects = eb
                            .put_block(block)
                            .event(move |result| Event::PutBlockResult { block_hash, result });

                        let deploy = Deploy::new(rng.gen());
                        let deploy_hash = *deploy.id();
                        effects.extend(eb.put_deploy(deploy).event(move |result| {
                            Event::PutDeployResult {
                                deploy_hash,
                                result,
                            }
                        }));
                    } else {
                        let block_hash = BlockHash::new(hash::hash(&[rng.gen::<u8>()]));
                        effects = eb
                            .get_block(block_hash)
                            .event(move |result| Event::GetBlockResult { block_hash, result });
                        effects.extend(eb.get_block_header(block_hash).event(move |result| {
                            Event::GetBlockHeaderResult { block_hash, result }
                        }));

                        let deploy_hash = DeployHash::new(hash::hash(&[rng.gen::<u8>()]));
                        effects.extend(eb.get_deploy(deploy_hash).event(move |result| {
                            Event::GetDeployResult {
                                deploy_hash,
                                result,
                            }
                        }));
                        effects.extend(eb.get_deploy_header(deploy_hash).event(move |result| {
                            Event::GetDeployHeaderResult {
                                deploy_hash,
                                result,
                            }
                        }));
                    }
                    effects
                }
                Event::PutBlockResult { block_hash, result } => {
                    if let Err(error) = result {
                        info!(
                            "consumer knows {} has failed to be stored: {}",
                            block_hash, error
                        );
                    } else {
                        info!("consumer knows {} has been stored.", block_hash);
                    }
                    Self::set_timeout(eb)
                }
                Event::GetBlockResult { block_hash, result } => {
                    match result {
                        Ok(block) => info!("consumer got {}", block),
                        Err(error) => {
                            info!("consumer failed to get block {}: {}", block_hash, error)
                        }
                    }
                    Self::set_timeout(eb)
                }
                Event::GetBlockHeaderResult { block_hash, result } => {
                    match result {
                        Ok(block_header) => info!("consumer got {}", block_header),
                        Err(error) => info!(
                            "consumer failed to get block header {}: {}",
                            block_hash, error
                        ),
                    }
                    Multiple::new()
                }
                Event::PutDeployResult {
                    deploy_hash,
                    result,
                } => {
                    if let Err(error) = result {
                        info!(
                            "consumer knows {} has failed to be stored: {}",
                            deploy_hash, error
                        );
                    } else {
                        info!("consumer knows {} has been stored.", deploy_hash);
                    }
                    Multiple::new()
                }
                Event::GetDeployResult {
                    deploy_hash,
                    result,
                } => {
                    match result {
                        Ok(deploy) => info!("consumer got {}", deploy),
                        Err(error) => {
                            info!("consumer failed to get deploy {}: {}", deploy_hash, error)
                        }
                    }
                    Multiple::new()
                }
                Event::GetDeployHeaderResult {
                    deploy_hash,
                    result,
                } => {
                    match result {
                        Ok(deploy_header) => info!("consumer got {}", deploy_header),
                        Err(error) => info!(
                            "consumer failed to get deploy header {}: {}",
                            deploy_hash, error
                        ),
                    }
                    Multiple::new()
                }
            }
        }
    }

    impl StorageConsumer {
        pub(crate) fn new<REv>(eb: EffectBuilder<REv>) -> (Self, Multiple<Effect<Event>>)
        where
            REv: From<Box<Event>> + Send + From<StorageRequest<Storage>>,
        {
            (Self {}, Self::set_timeout(eb))
        }

        fn set_timeout<REv>(eb: EffectBuilder<REv>) -> Multiple<Effect<Event>>
        where
            REv: From<Box<Event>> + Send + From<StorageRequest<Storage>>,
        {
            eb.set_timeout(Duration::from_millis(100))
                .event(|_| Event::Trigger)
        }
    }
}
