//! Reactor used to initialize a node.

use cosy_macro::reactor;

use crate::{
    components::{
        small_network::NodeId,
        storage::{Storage, StorageType},
    },
    protocol::Message,
    reactor::validator,
    utils::WithDir,
};

reactor!(Initializer {
  type Config = WithDir<validator::Config>;

  components: {
    chainspec = @ChainspecLoader(cfg
           .value()
           .node
           .chainspec_config_path
           .clone()
           .load(cfg.dir())
           .expect("TODO: return proper error when chainspec cannot be loaded"), effect_builder);
    storage = Storage(cfg.map_ref(|cfg| cfg.storage.clone()));
    contract_runtime = ContractRuntime(cfg.map_ref(|cfg| cfg.storage.clone()), &cfg.value().contract_runtime, registry);
  }

  events: {
    storage = Event<Storage>;
  }

  requests: {
    StorageRequest<Storage> -> storage;
    ContractRuntimeRequest -> contract_runtime;

    // No network traffic during initialization, just discard.
    // TODO: Allow for "hard" discard, resulting in a crash?
    NetworkRequest<NodeId, Message> -> !;
  }

  announcements: {
    // We neither send nor process announcements.
  }
});

// TODO: Metrics
// TODO: is_stopped

impl Initializer {
    /// Returns whether the initialization process completed successfully or not.
    pub fn stopped_successfully(&self) -> bool {
        self.chainspec.stopped_successfully()
    }
}
