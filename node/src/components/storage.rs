mod chainspec_store;
mod config;
mod error;
mod event;
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

use futures::TryFutureExt;
use rand::{CryptoRng, Rng};
use semver::Version;
use serde::{de::DeserializeOwned, Serialize};
use smallvec::smallvec;
use tokio::task;
use tracing::{debug, error};

use crate::{
    components::{chainspec_loader::Chainspec, small_network::NodeId, Component},
    effect::{
        requests::{NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    reactor::validator::Message,
    types::{Block, Deploy, Item},
};
// Seems to be a false positive.
#[allow(unreachable_pub)]
pub use config::Config;
// Seems to be a false positive.
use chainspec_store::ChainspecStore;
#[allow(unreachable_pub)]
pub use error::Error;
pub(crate) use error::Result;
pub use event::Event;
use in_mem_chainspec_store::InMemChainspecStore;
use in_mem_store::InMemStore;
use lmdb_chainspec_store::LmdbChainspecStore;
use lmdb_store::LmdbStore;
use store::{Multiple, Store};

pub(crate) type Storage = LmdbStorage<Block, Deploy>;

pub(crate) type DeployResults<S> = Multiple<Option<<S as StorageType>::Deploy>>;
pub(crate) type DeployHashes<S> = Multiple<<<S as StorageType>::Deploy as Value>::Id>;
pub(crate) type DeployHeaderResults<S> =
    Multiple<Option<<<S as StorageType>::Deploy as Value>::Header>>;

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
    type Deploy: Value + Item;

    fn block_store(&self) -> Arc<dyn Store<Value = Self::Block>>;
    fn deploy_store(&self) -> Arc<dyn Store<Value = Self::Deploy>>;
    fn chainspec_store(&self) -> Arc<dyn ChainspecStore>;
    fn new(config: &Config) -> Result<Self>
    where
        Self: Sized;

    fn get_deploy_for_peer<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
        deploy_hash: <Self::Deploy as Value>::Id,
        peer: NodeId,
    ) -> Effects<Event<Self>>
    where
        REv: From<NetworkRequest<NodeId, Message>> + Send,
        Self: Sized,
    {
        let deploy_store = self.deploy_store();
        let deploy_hashes = smallvec![deploy_hash];
        async move {
            task::spawn_blocking(move || deploy_store.get(deploy_hashes))
                .await
                .expect("should run")
                .pop()
                .expect("can only contain one result")
        }
        .map_err(move |error| debug!("failed to get {} for {}: {}", deploy_hash, peer, error))
        .and_then(move |maybe_deploy| async move {
            match maybe_deploy {
                Some(deploy) => match Message::new_get_response(&deploy) {
                    Ok(message) => effect_builder.send_message(peer, message).await,
                    Err(error) => error!("failed to create get-response: {}", error),
                },
                None => debug!("failed to get {} for {}", deploy_hash, peer),
            }
            Ok(())
        })
        .ignore()
    }

    fn put_block(&self, block: Box<Self::Block>, responder: Responder<bool>) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let block_store = self.block_store();
        let block_hash = *block.id();
        async move {
            let result = task::spawn_blocking(move || block_store.put(*block))
                .await
                .expect("should run")
                .unwrap_or_else(|error| panic!("failed to put {}: {}", block_hash, error));
            responder.respond(result).await
        }
        .ignore()
    }

    fn get_block(
        &self,
        block_hash: <Self::Block as Value>::Id,
        responder: Responder<Option<Self::Block>>,
    ) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let block_store = self.block_store();
        async move {
            let mut results = task::spawn_blocking(move || block_store.get(smallvec![block_hash]))
                .await
                .expect("should run");
            let result = results
                .pop()
                .expect("can only contain one result")
                .unwrap_or_else(|error| panic!("failed to get {}: {}", block_hash, error));
            responder.respond(result).await
        }
        .ignore()
    }

    fn get_block_header(
        &self,
        block_hash: <Self::Block as Value>::Id,
        responder: Responder<Option<<Self::Block as Value>::Header>>,
    ) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let block_store = self.block_store();
        async move {
            let mut results =
                task::spawn_blocking(move || block_store.get_headers(smallvec![block_hash]))
                    .await
                    .expect("should run");
            let result = results
                .pop()
                .expect("can only contain one result")
                .unwrap_or_else(|error| panic!("failed to get header {}: {}", block_hash, error));
            responder.respond(result).await
        }
        .ignore()
    }

    fn put_deploy(
        &self,
        deploy: Box<Self::Deploy>,
        responder: Responder<bool>,
    ) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let deploy_store = self.deploy_store();
        let deploy_hash = *Value::id(&*deploy);
        async move {
            let result = task::spawn_blocking(move || deploy_store.put(*deploy))
                .await
                .expect("should run")
                .unwrap_or_else(|error| panic!("failed to put {}: {}", deploy_hash, error));
            responder.respond(result).await;
        }
        .ignore()
    }

    fn get_deploys(
        &self,
        deploy_hashes: DeployHashes<Self>,
        responder: Responder<DeployResults<Self>>,
    ) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let deploy_store = self.deploy_store();
        async move {
            let results = task::spawn_blocking(move || deploy_store.get(deploy_hashes))
                .await
                .expect("should run")
                .into_iter()
                .map(|result| {
                    result.unwrap_or_else(|error| panic!("failed to get deploy: {}", error))
                })
                .collect();
            responder.respond(results).await
        }
        .ignore()
    }

    fn get_deploy_headers(
        &self,
        deploy_hashes: DeployHashes<Self>,
        responder: Responder<DeployHeaderResults<Self>>,
    ) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let deploy_store = self.deploy_store();
        async move {
            let results = task::spawn_blocking(move || deploy_store.get_headers(deploy_hashes))
                .await
                .expect("should run")
                .into_iter()
                .map(|result| {
                    result.unwrap_or_else(|error| panic!("failed to get deploy header: {}", error))
                })
                .collect();
            responder.respond(results).await
        }
        .ignore()
    }

    fn list_deploys(
        &self,
        responder: Responder<Vec<<Self::Deploy as Value>::Id>>,
    ) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let deploy_store = self.deploy_store();
        async move {
            let result = task::spawn_blocking(move || deploy_store.ids())
                .await
                .expect("should run")
                .unwrap_or_else(|error| panic!("failed to list deploys: {}", error));
            responder.respond(result).await
        }
        .ignore()
    }

    fn put_chainspec(
        &self,
        chainspec: Box<Chainspec>,
        responder: Responder<()>,
    ) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let chainspec_store = self.chainspec_store();
        async move {
            task::spawn_blocking(move || chainspec_store.put(*chainspec))
                .await
                .expect("should run")
                .unwrap_or_else(|error| panic!("failed to put chainspec: {}", error));
            responder.respond(()).await
        }
        .ignore()
    }

    fn get_chainspec(
        &self,
        version: Version,
        responder: Responder<Option<Chainspec>>,
    ) -> Effects<Event<Self>>
    where
        Self: Sized,
    {
        let chainspec_store = self.chainspec_store();
        async move {
            let result = task::spawn_blocking(move || chainspec_store.get(version))
                .await
                .expect("should run")
                .unwrap_or_else(|error| panic!("failed to get chainspec: {}", error));
            responder.respond(result).await
        }
        .ignore()
    }
}

impl<REv, R, S> Component<REv, R> for S
where
    REv: From<NetworkRequest<NodeId, Message>> + Send,
    R: Rng + CryptoRng + ?Sized,
    S: StorageType,
    Self: Sized + 'static,
{
    type Event = Event<S>;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::GetDeployForPeer { deploy_hash, peer } => {
                self.get_deploy_for_peer(effect_builder, deploy_hash, peer)
            }
            Event::Request(StorageRequest::PutBlock { block, responder }) => {
                self.put_block(block, responder)
            }
            Event::Request(StorageRequest::GetBlock {
                block_hash,
                responder,
            }) => self.get_block(block_hash, responder),
            Event::Request(StorageRequest::GetBlockHeader {
                block_hash,
                responder,
            }) => self.get_block_header(block_hash, responder),
            Event::Request(StorageRequest::PutDeploy { deploy, responder }) => {
                self.put_deploy(deploy, responder)
            }
            Event::Request(StorageRequest::GetDeploys {
                deploy_hashes,
                responder,
            }) => self.get_deploys(deploy_hashes, responder),
            Event::Request(StorageRequest::GetDeployHeaders {
                deploy_hashes,
                responder,
            }) => self.get_deploy_headers(deploy_hashes, responder),
            Event::Request(StorageRequest::ListDeploys { responder }) => {
                self.list_deploys(responder)
            }
            Event::Request(StorageRequest::PutChainspec {
                chainspec,
                responder,
            }) => self.put_chainspec(chainspec, responder),
            Event::Request(StorageRequest::GetChainspec { version, responder }) => {
                self.get_chainspec(version, responder)
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
impl<B: Value + 'static, D: Value + Item + 'static> StorageType for InMemStorage<B, D> {
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
impl<B: Value + 'static, D: Value + Item + 'static> StorageType for LmdbStorage<B, D> {
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
