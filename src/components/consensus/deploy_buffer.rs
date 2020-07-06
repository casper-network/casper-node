use std::collections::{HashMap, HashSet};

use crate::types::{BlockHash, DeployHash, DeployHeader};

#[derive(Debug, Clone, Default)]
pub(crate) struct DeployBuffer {
    collected_deploys: HashMap<DeployHash, DeployHeader>,
    processed: HashMap<BlockHash, HashMap<DeployHash, DeployHeader>>,
    finalized: HashMap<BlockHash, HashMap<DeployHash, DeployHeader>>,
}

impl DeployBuffer {
    /// Creates a new, empty deploy buffer instance.
    pub(crate) fn new() -> Self {
        Default::default()
    }

    /// Add a deploy to the deploy buffer.
    ///
    /// Returns `false` if the deploy has been rejected.
    pub(crate) fn add_deploy(&mut self, hash: DeployHash, deploy: DeployHeader) -> bool {
        // only add the deploy if it isn't contained in a finalized block
        if !self
            .finalized
            .values()
            .any(|block| block.contains_key(&hash))
        {
            self.collected_deploys.insert(hash, deploy);
            true
        } else {
            false
        }
    }

    /// `blocks` contains the ancestors that haven't been finalized yet - we exclude all the
    /// deploys from the finalized blocks by default.
    pub(crate) fn remaining_deploys(
        &mut self,
        current_instant: u64,
        max_ttl: u32,
        max_block_size_bytes: u64,
        max_gas_limit: u64,
        max_dependencies: u8,
        past: &HashSet<BlockHash>,
    ) -> HashSet<DeployHash> {
        let past_deploys = past
            .iter()
            .filter_map(|block_hash| self.processed.get(block_hash))
            .chain(self.finalized.values())
            .flat_map(|deploys| deploys.keys())
            .collect::<HashSet<_>>();
        // deploys_to_return = all deploys in collected_deploys that aren't in finalized blocks or
        // processed blocks from the set `past`
        self.collected_deploys
            .iter()
            .filter(|&(hash, deploy)| {
                self.is_deploy_valid(
                    deploy,
                    current_instant,
                    max_ttl,
                    max_dependencies,
                    &past_deploys,
                ) && !past_deploys.contains(hash)
            })
            .map(|(hash, deploy)| *hash)
            .collect::<HashSet<_>>()
        // TODO: check gas and block size limits
    }

    fn is_deploy_valid(
        &self,
        deploy: &DeployHeader,
        current_instant: u64,
        max_ttl: u32,
        max_dependencies: u8,
        past_deploys: &HashSet<&DeployHash>,
    ) -> bool {
        let all_deps_resolved = || {
            deploy
                .dependencies
                .iter()
                .all(|dep| past_deploys.contains(dep))
        };
        let ttl_valid = deploy.ttl_millis <= max_ttl;
        let timestamp_valid = deploy.timestamp <= current_instant;
        let deploy_valid = deploy.timestamp + deploy.ttl_millis as u64 >= current_instant;
        let num_deps_valid = deploy.dependencies.len() <= max_dependencies as usize;
        ttl_valid && timestamp_valid && deploy_valid && num_deps_valid && all_deps_resolved()
    }

    pub(crate) fn added_block(&mut self, block: BlockHash, deploys: HashSet<DeployHash>) {
        let deploy_map = deploys
            .iter()
            .filter_map(|deploy_hash| {
                self.collected_deploys
                    .get(deploy_hash)
                    .map(|deploy| (*deploy_hash, deploy.clone()))
            })
            .collect();
        self.collected_deploys
            .retain(|deploy_hash, _| !deploys.contains(deploy_hash));
        self.processed.insert(block, deploy_map);
    }

    pub(crate) fn finalized_block(&mut self, block: BlockHash) {
        if let Some(deploys) = self.processed.remove(&block) {
            self.collected_deploys
                .retain(|deploy_hash, _| !deploys.contains_key(deploy_hash));
            self.finalized.insert(block, deploys);
        } else {
            panic!("finalized block that hasn't been processed!");
        }
    }

    pub(crate) fn orphaned_block(&mut self, block: BlockHash) {
        if let Some(deploys) = self.processed.remove(&block) {
            self.collected_deploys.extend(deploys);
        } else {
            panic!("orphaned block that hasn't been processed!");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use rand::random;

    use super::DeployBuffer;
    use crate::{
        crypto::{asymmetric_key::PublicKey, hash::hash},
        types::{BlockHash, DeployHash, DeployHeader},
    };

    fn generate_deploy(timestamp: u64, ttl: u32) -> (DeployHash, DeployHeader) {
        let deploy_hash = DeployHash::new(hash(random::<[u8; 16]>()));
        let deploy = DeployHeader {
            account: PublicKey::new_ed25519([1; PublicKey::ED25519_LENGTH]).unwrap(),
            timestamp,
            gas_price: 10,
            body_hash: hash(random::<[u8; 16]>()),
            ttl_millis: ttl,
            dependencies: vec![],
            chain_name: "chain".to_string(),
        };
        (deploy_hash, deploy)
    }

    fn remaining_deploys(
        buffer: &mut DeployBuffer,
        time: u64,
        blocks: &HashSet<BlockHash>,
    ) -> HashSet<DeployHash> {
        let max_ttl = 200u32;
        // TODO:
        let max_block_size = 0u64;
        let max_gas_limit = 0u64;
        let max_dependencies = 1u8;

        buffer.remaining_deploys(
            time,
            max_ttl,
            max_block_size,
            max_gas_limit,
            max_dependencies,
            blocks,
        )
    }

    #[test]
    fn add_and_take_deploys() {
        let creation_time = 100u64;
        let ttl = 100u32;
        let block_time1 = 80u64;
        let block_time2 = 120u64;
        let block_time3 = 220u64;

        let no_blocks = HashSet::new();
        let mut buffer = DeployBuffer::new();
        let (hash1, deploy1) = generate_deploy(creation_time, ttl);
        let (hash2, deploy2) = generate_deploy(creation_time, ttl);
        let (hash3, deploy3) = generate_deploy(creation_time, ttl);
        let (hash4, deploy4) = generate_deploy(creation_time, ttl);

        assert!(remaining_deploys(&mut buffer, block_time2, &no_blocks).is_empty());

        // add two deploys
        buffer.add_deploy(hash1, deploy1);
        buffer.add_deploy(hash2, deploy2.clone());

        // if we try to create a block with a timestamp that is too early, we shouldn't get any
        // deploys
        assert!(remaining_deploys(&mut buffer, block_time1, &no_blocks).is_empty());

        // if we try to create a block with a timestamp that is too late, we shouldn't get any
        // deploys, either
        assert!(remaining_deploys(&mut buffer, block_time3, &no_blocks).is_empty());

        // take the deploys out
        let deploys = remaining_deploys(&mut buffer, block_time2, &no_blocks);

        assert_eq!(deploys.len(), 2);
        assert!(deploys.contains(&hash1));
        assert!(deploys.contains(&hash2));

        // the deploys should not have been removed yet
        assert!(!remaining_deploys(&mut buffer, block_time2, &no_blocks).is_empty());

        // the two deploys will be included in block 1
        let block_hash1 = BlockHash::new(hash(random::<[u8; 16]>()));
        buffer.added_block(block_hash1, deploys);

        // the deploys should have been removed now
        assert!(remaining_deploys(&mut buffer, block_time2, &no_blocks).is_empty());

        let mut blocks = HashSet::new();
        blocks.insert(block_hash1);

        assert!(remaining_deploys(&mut buffer, block_time2, &blocks).is_empty());

        // try adding the same deploy again
        buffer.add_deploy(hash2, deploy2.clone());

        // it shouldn't be returned if we include block 1 in the past blocks
        assert!(remaining_deploys(&mut buffer, block_time2, &blocks).is_empty());
        // ...but it should be returned if we don't include it
        assert!(remaining_deploys(&mut buffer, block_time2, &no_blocks).len() == 1);

        // the previous check removed the deploy from the buffer, let's re-add it
        buffer.add_deploy(hash2, deploy2);

        // finalize the block
        buffer.finalized_block(block_hash1);

        // add more deploys
        buffer.add_deploy(hash3, deploy3);
        buffer.add_deploy(hash4, deploy4);

        let deploys = remaining_deploys(&mut buffer, block_time2, &no_blocks);

        // since block 1 is now finalized, deploy2 shouldn't be among the ones returned
        assert_eq!(deploys.len(), 2);
        assert!(deploys.contains(&hash3));
        assert!(deploys.contains(&hash4));
    }

    #[test]
    fn test_deploy_dependencies() {
        let creation_time = 100u64;
        let ttl = 100u32;
        let block_time = 120u64;

        let (hash1, deploy1) = generate_deploy(creation_time, ttl);
        let (hash2, mut deploy2) = generate_deploy(creation_time, ttl);
        // let deploy2 depend on deploy1
        deploy2.dependencies = vec![hash1];

        let mut blocks = HashSet::new();
        let mut buffer = DeployBuffer::new();

        // add deploy2
        buffer.add_deploy(hash2, deploy2);

        // deploy2 has an unsatisfied dependency
        assert!(remaining_deploys(&mut buffer, block_time, &blocks).is_empty());

        // add deploy1
        buffer.add_deploy(hash1, deploy1);

        let deploys = remaining_deploys(&mut buffer, block_time, &blocks);
        // only deploy1 should be returned, as it has no dependencies
        assert_eq!(deploys.len(), 1);
        assert!(deploys.contains(&hash1));

        // the deploy will be included in block 1
        let block_hash1 = BlockHash::new(hash(random::<[u8; 16]>()));
        buffer.added_block(block_hash1, deploys);
        blocks.insert(block_hash1);

        let deploys2 = remaining_deploys(&mut buffer, block_time, &blocks);
        // `blocks` contains a block that contains deploy1 now, so we should get deploy2
        assert_eq!(deploys2.len(), 1);
        assert!(deploys2.contains(&hash2));
    }
}
