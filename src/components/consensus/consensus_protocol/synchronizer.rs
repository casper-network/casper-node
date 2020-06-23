use super::{
    protocol_state::{ProtocolState, Vertex, VertexId},
    NodeId,
};
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    hash::Hash,
};
use serde::export::PhantomData;

/// Note that we might be requesting download of the duplicate element
/// (one that had requested for earlier) but with a different node.
/// The assumption is that a downloading layer will collect different node IDs as alternative
/// sources and use different address in the case of download failures.
pub(crate) enum SynchronizerEffect<VId, V, C> {
    /// Effect for the reactor to download missing vertex.
    RequestVertex(NodeId, VId),
    /// Effect for the reactor to download missing consensus values (a deploy for example).
    RequestConsensusValues(NodeId, Vec<C>),
    /// Effect for the reactor to requeue a vertex once its dependencies are downloaded.
    RequeueVertex(Vec<V>),
    RequestedVertexResponse(Option<V>),
    /// Vertex addition failed for some reason.
    /// TODO: Differentiate from attributable failures.
    InvalidVertex(V, NodeId, anyhow::Error),
}

/// Structure that tracks which vertices wait for what consensus value dependencies.
pub(crate) struct ConsensusValueDependencies<C: Hash + PartialEq + Eq, Id: Hash + PartialEq + Eq> {
    // Multiple vertices can be dependent on the same consensus value.
    cv_to_set: HashMap<C, Vec<Id>>,
    // Each vertex can be depending on multiple consensus values.
    id_to_group: HashMap<Id, HashSet<C>>,
}

impl<C, Id> ConsensusValueDependencies<C, Id>
where
    C: Hash + PartialEq + Eq + Clone,
    Id: Hash + PartialEq + Eq + Clone,
{
    fn new() -> Self {
        ConsensusValueDependencies {
            cv_to_set: HashMap::new(),
            id_to_group: HashMap::new(),
        }
    }

    /// Adds a consensus value dependency.
    fn add(&mut self, c: C, id: Id) {
        self.cv_to_set
            .entry(c.clone())
            .or_default()
            .push(id.clone());
        self.id_to_group.entry(id).or_default().insert(c);
    }

    /// Remove a consensus value from dependencies.
    /// Call when it's downloaded/synchronized.
    /// Returns vertices that were waiting on it.
    fn remove(&mut self, c: C) -> Vec<Id> {
        // Get list of vertices that are dependent for the consensus value.
        match self.cv_to_set.remove(&c) {
            None => Vec::new(),
            Some(dependent_vertices) => {
                // Remove the consensus value from the set of values each vertex is waiting for.
                dependent_vertices
                    .into_iter()
                    .filter(|vertex| {
                        if let Entry::Occupied(mut consensus_values) =
                            self.id_to_group.entry(vertex.clone())
                        {
                            consensus_values.get_mut().remove(&c);
                            if consensus_values.get().is_empty() {
                                consensus_values.remove();
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    })
                    .collect()
            }
        }
    }
}

pub(crate) struct DagSynchronizerState<VId, V, C, P>
where
    C: Hash + PartialEq + Eq,
    VId: Hash + PartialEq + Eq,
{
    consensus_value_deps: ConsensusValueDependencies<C, VId>,
    // Tracks which vertices are still waiting for its vertex dependencies to be downloaded.
    // Since a vertex can have multiple vertices depend on it, downloading single vertex
    // can "release" more than one new vertex to be requeued to the reactor.
    //TODO: Wrap the following with a struct that will keep the details hidden.
    vertex_dependants: HashMap<VId, Vec<VId>>,
    vertex_by_vid: HashMap<VId, V>,
    _protocol_state: PhantomData<P>,
}

impl<C, VId, V, P> DagSynchronizerState<VId, V, C, P>
where
    C: Hash + PartialEq + Eq + Clone,
    VId: VertexId + Clone + Hash + Eq + PartialEq,
    V: Vertex<C, VId> + Clone,
    P: ProtocolState<VId, V>,
{
    fn new() -> Self {
        DagSynchronizerState {
            consensus_value_deps: ConsensusValueDependencies::new(),
            vertex_dependants: HashMap::new(),
            vertex_by_vid: HashMap::new(),
            _protocol_state: PhantomData,
        }
    }

    pub(crate) fn get_vertex(
        &self,
        v_id: VId,
        protocol_state: &P,
    ) -> Result<SynchronizerEffect<VId, V, C>, anyhow::Error> {
        protocol_state
            .get_vertex(v_id)
            .map_err(|err| anyhow::anyhow!("{:?}", err)) //TODO: Improve error reporting
            .map(SynchronizerEffect::RequestedVertexResponse)
    }

    pub(crate) fn add_vertex(
        &mut self,
        sender: NodeId,
        v: V,
        protocol_state: &mut P,
    ) -> Result<SynchronizerEffect<VId, V, C>, anyhow::Error> {
        match protocol_state.add_vertex(v.clone()) {
            Ok(Some(missing_vid)) => {
                self.add_vertex_dependency(missing_vid.clone(), v);
                Ok(SynchronizerEffect::RequestVertex(sender, missing_vid))
            }
            Ok(None) => {
                let vertices_with_completed_dependencies = self.complete_vertex_dependency(v.id());
                Ok(SynchronizerEffect::RequeueVertex(
                    vertices_with_completed_dependencies,
                ))
            }
            Err(error) => Err(anyhow::anyhow!("{:?}", error)),
        }
    }

    fn add_vertex_dependency(&mut self, v_id: VId, v: V) {
        let dependant_id = v.id();
        self.vertex_by_vid.entry(dependant_id.clone()).or_insert(v);
        self.vertex_dependants
            .entry(v_id)
            .or_insert_with(Vec::new)
            .push(dependant_id);
    }

    fn add_consensus_value_dependency(&mut self, c: C, v: &V) {
        let dependant_id = v.id();
        self.vertex_by_vid
            .entry(dependant_id.clone())
            .or_insert_with(|| v.clone());
        self.consensus_value_deps.add(c, dependant_id)
    }

    /// Complete a vertex dependency (called when that vertex is downloaded from another node and
    /// persisted). Returns list of vertices that were waiting on that vertex dependency.
    /// Vertices returned have all of its dependencies completed - i.e. are not waiting for
    /// anything else.
    fn complete_vertex_dependency(&mut self, v_id: VId) -> Vec<V> {
        match self.vertex_dependants.remove(&v_id) {
            None => Vec::new(),
            Some(dependants) => self.get_vertices_by_id(dependants),
        }
    }

    /// Complete a consensus value dependency (called when that C is downloaded from another node).
    /// Returns list of vertices that were waiting on the completion of that consensus value.
    /// Vertices returned have all of its dependencies completed - i.e. are not waiting for anything
    /// else.
    fn complete_consensus_value_dependency(&mut self, c: C) -> Vec<V> {
        let dependants = self.consensus_value_deps.remove(c);
        if dependants.is_empty() {
            Vec::new()
        } else {
            self.get_vertices_by_id(dependants)
        }
    }

    /// Helper method for returning list of vertices by its ID.
    fn get_vertices_by_id(&mut self, vertex_ids: Vec<VId>) -> Vec<V> {
        vertex_ids
            .into_iter()
            .filter_map(|vertex_id| self.vertex_by_vid.remove(&vertex_id))
            .collect()
    }

    /// Synchronizes the consensus value the vertex is introducing to the protocol state.
    /// It may be a single deploy, list of deploys, an integer value etc.
    /// Implementations will know which values are missing
    /// (ex. deploys in the local deploy buffer vs new deploys introduced by the block).
    /// Node passed in is the one that proposed the original vertex. It should also have the missing
    /// dependency.
    fn sync_consensus_values(
        &mut self,
        node: NodeId,
        c: Vec<C>,
        v: V,
    ) -> SynchronizerEffect<VId, V, C> {
        c.iter()
            .for_each(|c| self.add_consensus_value_dependency(c.clone(), &v));

        SynchronizerEffect::RequestConsensusValues(node, c)
    }

    /// Synchronizes the dependency (single) of a newly received vertex.
    /// In practice, this method will produce an effect that will be passed on to the reactor for
    /// handling. Node passed in is the one that proposed the original vertex. It should also
    /// have the missing dependency.
    fn sync_dependency(
        &mut self,
        node: NodeId,
        missing_dependency: VId,
        new_vertex: V,
    ) -> SynchronizerEffect<VId, V, C> {
        self.add_vertex_dependency(missing_dependency.clone(), new_vertex);
        SynchronizerEffect::RequestVertex(node, missing_dependency)
    }

    /// Must be called after consensus successfully handles the new vertex.
    /// That's b/c there might be other vertices that depend on this one and are waiting in a queue.
    fn on_vertex_synced(&mut self, v: VId) -> Vec<SynchronizerEffect<VId, V, C>> {
        let completed_dependencies = self.complete_vertex_dependency(v);
        completed_dependencies
            .into_iter()
            .map(|v| SynchronizerEffect::RequeueVertex(vec![v]))
            .collect()
    }

    fn on_consensus_value_synced(&mut self, c: C) -> Vec<SynchronizerEffect<VId, V, C>> {
        let completed_dependencies = self.complete_consensus_value_dependency(c);
        completed_dependencies
            .into_iter()
            .map(|v| SynchronizerEffect::RequeueVertex(vec![v]))
            .collect()
    }
}
