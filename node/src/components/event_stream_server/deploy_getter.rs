#[cfg(test)]
use std::collections::HashMap;
use std::sync::Arc;

use datasize::DataSize;
use once_cell::sync::OnceCell;
use tracing::error;

use crate::{
    effect::EffectBuilder,
    reactor::{joiner::JoinerEvent, participating::ParticipatingEvent},
    types::{Deploy, DeployHash, DeployWithFinalizedApprovals},
};

/// A struct holding the two effect builders in use during the lifetime of the event stream
/// server.
///
/// Initially, only the joiner one is populated.  Once the joiner reactor gives way to the
/// participating reactor, the participating effect builder is set.
#[derive(Clone, Default, Debug)]
struct CommonEffectBuilder {
    joiner: OnceCell<EffectBuilder<JoinerEvent>>,
    participating: OnceCell<EffectBuilder<ParticipatingEvent>>,
}

/// A struct to enable the event stream server tasks to fetch deploys from storage.
#[derive(Clone, Debug, DataSize)]
pub(crate) struct DeployGetter {
    #[data_size(skip)]
    effect_builder: Arc<CommonEffectBuilder>,
    #[cfg(test)]
    deploys: Arc<HashMap<DeployHash, Deploy>>,
}

impl DeployGetter {
    pub(crate) fn new(joiner_effect_builder: EffectBuilder<JoinerEvent>) -> Self {
        let effect_builder = CommonEffectBuilder::default();
        let _ = effect_builder.joiner.set(joiner_effect_builder);
        DeployGetter {
            effect_builder: Arc::new(effect_builder),
            #[cfg(test)]
            deploys: Arc::new(HashMap::new()),
        }
    }

    pub(super) fn set_participating_effect_builder(
        &self,
        effect_builder: EffectBuilder<ParticipatingEvent>,
    ) {
        if self
            .effect_builder
            .participating
            .set(effect_builder)
            .is_err()
        {
            error!("participating effect builder already set in deploy getter");
        }
    }

    /// Returns the requested `Deploy` by using the `CommonEffectBuilder` to retrieve it from
    /// the `Storage` component, or for tests by retrieving it from the test-only internal hash map.
    #[cfg_attr(test, allow(unreachable_code))]
    pub(super) async fn get(&self, deploy_hash: DeployHash) -> Option<Deploy> {
        #[cfg(test)]
        return self.get_test_deploy(deploy_hash);

        let deploy_hashes = vec![deploy_hash];
        let mut maybe_deploys =
            if let Some(participating_effect_builder) = self.effect_builder.participating.get() {
                participating_effect_builder
                    .get_deploys_from_storage(deploy_hashes)
                    .await
            } else if let Some(joiner_effect_builder) = self.effect_builder.joiner.get() {
                joiner_effect_builder
                    .get_deploys_from_storage(deploy_hashes)
                    .await
            } else {
                error!("no effect builder set in deploy getter");
                return None;
            };

        if maybe_deploys.len() != 1 {
            panic!("should return exactly one deploy");
        }
        maybe_deploys
            .pop()
            .unwrap()
            .map(DeployWithFinalizedApprovals::into_naive)
    }
}

#[cfg(test)]
impl DeployGetter {
    /// A test-only constructor taking the full set of `Deploy`s which will be available to the
    /// event stream server.
    pub(super) fn with_deploys(deploys: HashMap<DeployHash, Deploy>) -> Self {
        DeployGetter {
            effect_builder: Arc::new(CommonEffectBuilder::default()),
            deploys: Arc::new(deploys),
        }
    }

    /// A non-async, test-only getter for the given `Deploy`.
    pub(super) fn get_test_deploy(&self, deploy_hash: DeployHash) -> Option<Deploy> {
        self.deploys.get(&deploy_hash).cloned()
    }
}
