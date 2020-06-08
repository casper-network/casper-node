mod error;
mod in_mem_store;
mod store;

use std::{
    error::Error as StdError,
    fmt::{Debug, Display},
    hash::Hash,
    sync::Arc,
};

use rand::Rng;
use serde::{de::DeserializeOwned, Serialize};
use tokio::task;

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
