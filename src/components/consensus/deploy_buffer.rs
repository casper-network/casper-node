use std::collections::{HashMap, HashSet};

use crate::types::{BlockHash, DeployHash, DeployHeader};

#[derive(Debug, Clone, Default)]
pub(crate) struct DeployBuffer {
    collected_deploys: HashMap<DeployHash, DeployHeader>,
    processed: HashMap<BlockHash, HashMap<DeployHash, DeployHeader>>,
    finalized: HashMap<BlockHash, HashMap<DeployHash, DeployHeader>>,
}

impl DeployBuffer {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn add_deploy(&mut self, hash: DeployHash, deploy: DeployHeader) {
        // only add the deploy if it isn't contained in a finalized block
        if !self
            .finalized
            .values()
            .any(|block| block.contains_key(&hash))
        {
            self.collected_deploys.insert(hash, deploy);
        }
    }

    /// `blocks` contains the ancestors that haven't been finalized yet - we exclude all the
    /// deploys from the finalized blocks by default.
    pub(crate) fn remaining_deploys(
        &mut self,
        blocks: &HashSet<BlockHash>,
    ) -> HashMap<DeployHash, DeployHeader> {
        // deploys_to_return = all deploys in collected_deploys that aren't in finalized blocks or
        // processed blocks from the set `blocks`
        let deploys_to_return = blocks
            .iter()
            .filter_map(|block_hash| self.processed.get(block_hash))
            .chain(self.finalized.values())
            .fold(self.collected_deploys.clone(), |mut map, other_map| {
                map.retain(|deploy_hash, _deploy| !other_map.contains_key(deploy_hash));
                map
            });
        deploys_to_return
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
    use std::collections::HashSet;

    use rand::random;

    use super::DeployBuffer;
    use crate::{
        crypto::{asymmetric_key::PublicKey, hash::hash},
        types::{BlockHash, DeployHash, DeployHeader},
    };

    fn generate_deploy() -> (DeployHash, DeployHeader) {
        let deploy_hash = DeployHash::new(hash(random::<[u8; 16]>()));
        let deploy = DeployHeader {
            account: PublicKey::new_ed25519([1; PublicKey::ED25519_LENGTH]).unwrap(),
            timestamp: random(),
            gas_price: random(),
            body_hash: hash(random::<[u8; 16]>()),
            ttl_millis: random(),
            dependencies: vec![],
            chain_name: "chain".to_string(),
        };
        (deploy_hash, deploy)
    }

    #[test]
    fn add_and_take_deploys() {
        let no_blocks = HashSet::new();
        let mut buffer = DeployBuffer::new();
        let (hash1, deploy1) = generate_deploy();
        let (hash2, deploy2) = generate_deploy();
        let (hash3, deploy3) = generate_deploy();
        let (hash4, deploy4) = generate_deploy();

        assert!(buffer.remaining_deploys(&no_blocks).is_empty());

        // add two deploys
        buffer.add_deploy(hash1, deploy1);
        buffer.add_deploy(hash2, deploy2.clone());

        // take the deploys out
        let deploys = buffer.remaining_deploys(&no_blocks);

        assert_eq!(deploys.len(), 2);
        assert!(deploys.contains_key(&hash1));
        assert!(deploys.contains_key(&hash2));

        // the deploys should not have been removed yet
        assert!(!buffer.remaining_deploys(&no_blocks).is_empty());

        // the two deploys will be included in block 1
        let block_hash1 = BlockHash::new(hash(random::<[u8; 16]>()));
        buffer.added_block(block_hash1, deploys.keys().cloned().collect());

        // the deploys should have been removed now
        assert!(buffer.remaining_deploys(&no_blocks).is_empty());

        let mut blocks = HashSet::new();
        blocks.insert(block_hash1);

        assert!(buffer.remaining_deploys(&blocks).is_empty());

        // try adding the same deploy again
        buffer.add_deploy(hash2, deploy2.clone());

        // it shouldn't be returned if we include block 1 in the past blocks
        assert!(buffer.remaining_deploys(&blocks).is_empty());
        // ...but it should be returned if we don't include it
        assert!(buffer.remaining_deploys(&no_blocks).len() == 1);

        // the previous check removed the deploy from the buffer, let's re-add it
        buffer.add_deploy(hash2, deploy2);

        // finalize the block
        buffer.finalized_block(block_hash1);

        // add more deploys
        buffer.add_deploy(hash3, deploy3);
        buffer.add_deploy(hash4, deploy4);

        let deploys = buffer.remaining_deploys(&no_blocks);

        // since block 1 is now finalized, deploy2 shouldn't be among the ones returned
        assert_eq!(deploys.len(), 2);
        assert!(deploys.contains_key(&hash3));
        assert!(deploys.contains_key(&hash4));
    }
}
