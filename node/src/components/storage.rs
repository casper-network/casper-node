mod chainspec_store;
mod config;
mod error;
mod in_mem_chainspec_store;
mod in_mem_store;
mod lmdb_chainspec_store;
mod lmdb_store;
mod store;

use std::{
    fmt::{Debug, Display},
    fs,
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
// Seems to be a false positive.
#[allow(unreachable_pub)]
pub use config::Config;
// TODO - remove this once contract runtime's config doesn't need it any more.
pub(crate) use config::check_multiple_of_page_size;
// Seems to be a false positive.
use chainspec_store::ChainspecStore;
#[allow(unreachable_pub)]
pub use error::Error;
pub(crate) use error::Result;
use in_mem_chainspec_store::InMemChainspecStore;
use in_mem_store::InMemStore;
use lmdb_chainspec_store::LmdbChainspecStore;
use lmdb_store::LmdbStore;
use store::Store;

pub(crate) type Storage = LmdbStorage<Block, Deploy>;

const BLOCK_STORE_FILENAME: &str = "block_store.db";
const DEPLOY_STORE_FILENAME: &str = "deploy_store.db";
const CHAINSPEC_STORE_FILENAME: &str = "chainspec_store.db";

/// Trait defining the API for a value able to be held within the storage component.
pub trait Value: Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display {
    type Id: Copy
        + Clone
        + Ord
        + PartialOrd
        + Eq
        + PartialEq
        + Hash
        + Debug
        + Display
        + Serialize
        + DeserializeOwned
        + Send
        + Sync;
    /// A relatively small portion of the value, representing header info or metadata.
    type Header: Clone
        + Ord
        + PartialOrd
        + Eq
        + PartialEq
        + Hash
        + Debug
        + Display
        + Serialize
        + DeserializeOwned
        + Send
        + Sync;

    fn id(&self) -> &Self::Id;
    fn header(&self) -> &Self::Header;
    fn take_header(self) -> Self::Header;
}

/// Trait which will handle management of the various storage sub-components.
///
/// If this trait is ultimately only used for testing scenarios, we shouldn't need to expose it to
/// the reactor - it can simply use a concrete type which implements this trait.
pub trait StorageType {
    type Block: Value;
    type Deploy: Value;

    fn block_store(&self) -> Arc<dyn Store<Value = Self::Block>>;
    fn deploy_store(&self) -> Arc<dyn Store<Value = Self::Deploy>>;
    fn chainspec_store(&self) -> Arc<dyn ChainspecStore>;
    fn new(config: &Config) -> Result<Self>
    where
        Self: Sized;
}

impl<REv, S> Component<REv> for S
where
    S: StorageType,
    Self: Sized + 'static,
{
    type Event = StorageRequest<Self>;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
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
            StorageRequest::ListDeploys { responder } => {
                let deploy_store = self.deploy_store();
                async move {
                    let result = task::spawn_blocking(move || deploy_store.ids())
                        .await
                        .expect("should run");
                    responder.respond(result).await
                }
                .ignore()
            }
            StorageRequest::PutChainspec {
                chainspec,
                responder,
            } => {
                let chainspec_store = self.chainspec_store();
                async move {
                    let result = task::spawn_blocking(move || chainspec_store.put(*chainspec))
                        .await
                        .expect("should run");
                    responder.respond(result).await
                }
                .ignore()
            }
            StorageRequest::GetChainspec { version, responder } => {
                let chainspec_store = self.chainspec_store();
                async move {
                    let result = task::spawn_blocking(move || chainspec_store.get(version))
                        .await
                        .expect("should run");
                    responder.respond(result).await
                }
                .ignore()
            }
        }
    }
}

// Concrete type of `Storage` backed by in-memory stores.
#[derive(Debug)]
pub(crate) struct InMemStorage<B: Value, D: Value> {
    block_store: Arc<InMemStore<B>>,
    deploy_store: Arc<InMemStore<D>>,
    chainspec_store: Arc<InMemChainspecStore>,
}

#[allow(trivial_casts)]
impl<B: Value + 'static, D: Value + 'static> StorageType for InMemStorage<B, D> {
    type Block = B;
    type Deploy = D;

    fn block_store(&self) -> Arc<dyn Store<Value = B>> {
        Arc::clone(&self.block_store) as Arc<dyn Store<Value = B>>
    }

    fn deploy_store(&self) -> Arc<dyn Store<Value = D>> {
        Arc::clone(&self.deploy_store) as Arc<dyn Store<Value = D>>
    }

    fn chainspec_store(&self) -> Arc<dyn ChainspecStore> {
        Arc::clone(&self.chainspec_store) as Arc<dyn ChainspecStore>
    }

    fn new(_config: &Config) -> Result<Self> {
        Ok(InMemStorage {
            block_store: Arc::new(InMemStore::new()),
            deploy_store: Arc::new(InMemStore::new()),
            chainspec_store: Arc::new(InMemChainspecStore::new()),
        })
    }
}

// Concrete type of `Storage` backed by LMDB stores.
#[derive(Debug)]
pub struct LmdbStorage<B: Value, D: Value> {
    block_store: Arc<LmdbStore<B>>,
    deploy_store: Arc<LmdbStore<D>>,
    chainspec_store: Arc<LmdbChainspecStore>,
}

#[allow(trivial_casts)]
impl<B: Value + 'static, D: Value + 'static> StorageType for LmdbStorage<B, D> {
    type Block = B;
    type Deploy = D;

    fn new(config: &Config) -> Result<Self> {
        let path = config.path();
        fs::create_dir_all(&path).map_err(|error| Error::CreateDir {
            dir: path.display().to_string(),
            source: error,
        })?;

        let block_store_path = path.join(BLOCK_STORE_FILENAME);
        let deploy_store_path = path.join(DEPLOY_STORE_FILENAME);
        let chainspec_store_path = path.join(CHAINSPEC_STORE_FILENAME);

        let block_store = LmdbStore::new(block_store_path, config.max_block_store_size())?;
        let deploy_store = LmdbStore::new(deploy_store_path, config.max_deploy_store_size())?;
        let chainspec_store =
            LmdbChainspecStore::new(chainspec_store_path, config.max_chainspec_store_size())?;

        Ok(LmdbStorage {
            block_store: Arc::new(block_store),
            deploy_store: Arc::new(deploy_store),
            chainspec_store: Arc::new(chainspec_store),
        })
    }

    fn block_store(&self) -> Arc<dyn Store<Value = B>> {
        Arc::clone(&self.block_store) as Arc<dyn Store<Value = B>>
    }

    fn deploy_store(&self) -> Arc<dyn Store<Value = D>> {
        Arc::clone(&self.deploy_store) as Arc<dyn Store<Value = D>>
    }

    fn chainspec_store(&self) -> Arc<dyn ChainspecStore> {
        Arc::clone(&self.chainspec_store) as Arc<dyn ChainspecStore>
    }
}
