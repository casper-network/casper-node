use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
};

use super::super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NodeId(pub(crate) &'static str);

impl From<&'static str> for NodeId {
    fn from(s: &'static str) -> Self {
        Self(s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DagIndex(NodeId, usize);

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DagNode {
    pub(crate) data: Option<String>,
    pub(crate) parents: BTreeSet<DagIndex>,
}

#[derive(Clone, Debug)]
pub(crate) struct MissingDeps {
    to_be_returned: BTreeSet<DagIndex>,
    returned_deps: BTreeSet<DagIndex>,
}

impl MissingDeps {
    pub(crate) fn new(deps: BTreeSet<DagIndex>) -> Self {
        Self {
            to_be_returned: deps,
            returned_deps: BTreeSet::new(),
        }
    }
}

impl DependencySpec for MissingDeps {
    type DependencyDescription = DagIndex;
    type ItemId = DagIndex;
    type Item = DagNode;

    fn next_dependency(&mut self) -> Option<Self::DependencyDescription> {
        let mut deps = mem::take(&mut self.to_be_returned).into_iter();
        let next_dep = deps.next();
        self.to_be_returned = deps.collect();
        if let Some(dep) = next_dep {
            self.returned_deps.insert(dep);
        }
        next_dep
    }

    fn resolve_dependency(&mut self, dep: DagIndex) -> bool {
        self.to_be_returned.remove(&dep) || self.returned_deps.remove(&dep)
    }

    fn all_resolved(&self) -> bool {
        self.to_be_returned.is_empty() && self.returned_deps.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Dag {
    heads: BTreeMap<NodeId, DagIndex>,
    nodes: BTreeMap<DagIndex, DagNode>,
}

impl ProtocolState for Dag {
    type DepSpec = MissingDeps;

    fn get_dependency(&self, dep: &DagIndex) -> Option<ItemWithId<MissingDeps>> {
        self.nodes.get(dep).map(|node| ItemWithId {
            item_id: *dep,
            item: node.clone(),
        })
    }

    fn handle_new_item(
        &mut self,
        item_id: DagIndex,
        item: DagNode,
    ) -> HandleNewItemResult<MissingDeps> {
        match self.nodes.get(&item_id) {
            Some(existing_item) if existing_item == &item => HandleNewItemResult::Accepted,
            Some(_) => HandleNewItemResult::Invalid,
            None => {
                let missing_parents = item
                    .parents
                    .iter()
                    .filter(|&parent| !self.nodes.contains_key(parent))
                    .cloned()
                    .collect::<BTreeSet<_>>();
                if missing_parents.is_empty() {
                    self.nodes.insert(item_id, item);
                    self.heads.insert(item_id.0, item_id);
                    HandleNewItemResult::Accepted
                } else {
                    HandleNewItemResult::DependenciesMissing(MissingDeps::new(missing_parents))
                }
            }
        }
    }
}

impl Dag {
    pub(crate) fn new() -> Self {
        Self {
            heads: BTreeMap::new(),
            nodes: BTreeMap::new(),
        }
    }

    pub(crate) fn create_node(
        &mut self,
        our_id: NodeId,
        data: Option<String>,
    ) -> (DagIndex, DagNode) {
        let new_index = DagIndex(
            our_id,
            self.heads.get(&our_id).map(|index| index.1).unwrap_or(0),
        );
        let parents = self.heads.values().cloned().collect();
        let node = DagNode { data, parents };
        self.nodes.insert(new_index, node.clone());
        self.heads.insert(our_id, new_index);
        (new_index, node)
    }
}
