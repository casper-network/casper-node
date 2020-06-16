use std::collections::{HashMap, HashSet};

use crate::types::{BlockHash, DeployHeader};

#[derive(Debug, Clone, Default)]
pub(crate) struct DeployBuffer {
    collected_deploys: HashSet<DeployHeader>,
    processed: HashMap<BlockHash, HashSet<DeployHeader>>,
    finalized: HashMap<BlockHash, HashSet<DeployHeader>>,
}

impl DeployBuffer {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn add_deploy(&mut self, deploy: DeployHeader) {
        // only add the deploy if it isn't contained in a finalized block
        if !self.finalized.values().any(|block| block.contains(&deploy)) {
            self.collected_deploys.insert(deploy);
        }
    }

    /// `blocks` contains the ancestors that haven't been finalized yet - we exclude all the
    /// deploys from the finalized blocks by default.
    pub(crate) fn remaining_deploys(
        &mut self,
        blocks: &HashSet<BlockHash>,
    ) -> HashSet<DeployHeader> {
        // deploys_to_return = all deploys in collected_deploys that aren't in finalized blocks or
        // processed blocks from the set `blocks`
        let deploys_to_return = blocks
            .iter()
            .filter_map(|hash| self.processed.get(hash))
            .chain(self.finalized.values())
            .fold(self.collected_deploys.clone(), |mut set, other_set| {
                set.retain(|deploy| !other_set.contains(deploy));
                set
            });
        self.collected_deploys
            .retain(|deploy| !deploys_to_return.contains(deploy));
        deploys_to_return
    }

    pub(crate) fn added_block(&mut self, block: BlockHash, deploys: HashSet<DeployHeader>) {
        self.collected_deploys
            .retain(|deploy| !deploys.contains(deploy));
        self.processed.insert(block, deploys);
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
        types::{BlockHash, DeployHeader},
    };

    fn generate_deploy_header() -> DeployHeader {
        DeployHeader {
            account: PublicKey::new_ed25519([1; PublicKey::ED25519_LENGTH]).unwrap(),
            timestamp: random(),
            gas_price: random(),
            body_hash: hash(random::<[u8; 16]>()),
            ttl_millis: random(),
            dependencies: vec![],
            chain_name: "chain".to_string(),
        }
    }

    #[test]
    fn add_and_take_deploys() {
        let no_blocks = HashSet::new();
        let mut buffer = DeployBuffer::new();
        let deploy1 = generate_deploy_header();
        let deploy2 = generate_deploy_header();
        let deploy3 = generate_deploy_header();
        let deploy4 = generate_deploy_header();

        assert!(buffer.remaining_deploys(&no_blocks).is_empty());

        // add two deploys
        buffer.add_deploy(deploy1.clone());
        buffer.add_deploy(deploy2.clone());

        // take the deploys out
        let deploys = buffer.remaining_deploys(&no_blocks);

        assert_eq!(deploys.len(), 2);
        assert!(deploys.contains(&deploy1));
        assert!(deploys.contains(&deploy2));

        // the deploys should have been removed
        assert!(buffer.remaining_deploys(&no_blocks).is_empty());

        // the two deploys will be included in block 1
        let block_hash1 = BlockHash::new(hash(random::<[u8; 16]>()));
        buffer.added_block(block_hash1, deploys);

        let mut blocks = HashSet::new();
        blocks.insert(block_hash1);

        assert!(buffer.remaining_deploys(&blocks).is_empty());

        // try adding the same deploy again
        buffer.add_deploy(deploy2.clone());

        // it shouldn't be returned if we include block 1 in the past blocks
        assert!(buffer.remaining_deploys(&blocks).is_empty());
        // ...but it should be returned if we don't include it
        assert!(buffer.remaining_deploys(&no_blocks).len() == 1);

        // the previous check removed the deploy from the buffer, let's re-add it
        buffer.add_deploy(deploy2);

        // finalize the block
        buffer.finalized_block(block_hash1);

        // add more deploys
        buffer.add_deploy(deploy3.clone());
        buffer.add_deploy(deploy4.clone());

        let deploys = buffer.remaining_deploys(&no_blocks);

        // since block 1 is now finalized, deploy2 shouldn't be among the ones returned
        assert_eq!(deploys.len(), 2);
        assert!(deploys.contains(&deploy3));
        assert!(deploys.contains(&deploy4));
    }
}
