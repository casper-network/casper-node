use super::{
    protocol_state::{AddVertexOk, ProtocolState, VertexTrait},
    NodeId,
};
use std::collections::{hash_map::Entry, HashMap, HashSet};

/// Note that we might be requesting download of the duplicate element
/// (one that had requested for earlier) but with a different node.
/// The assumption is that a downloading layer will collect different node IDs as alternative
/// sources and use different address in the case of download failures.
pub(crate) enum SynchronizerEffect<V: VertexTrait> {
    /// Effect for the reactor to download missing vertex.
    RequestVertex(NodeId, V::Id),
    /// Effect for the reactor to download missing consensus values (a deploy for example).
    RequestConsensusValues(NodeId, Vec<V::Value>),
    /// Effect for the reactor to requeue a vertex once its dependencies are downloaded.
    RequeueVertex(Vec<V>),
    /// Vertex addition failed for some reason.
    /// TODO: Differentiate from attributable failures.
    InvalidVertex(V, NodeId, anyhow::Error),
}

/// Structure that tracks which vertices wait for what consensus value dependencies.
pub(crate) struct ConsensusValueDependencies<V: VertexTrait> {
    // Multiple vertices can be dependent on the same consensus value.
    cv_to_set: HashMap<V::Value, Vec<V::Id>>,
    // Each vertex can be depending on multiple consensus values.
    id_to_group: HashMap<V::Id, HashSet<V::Value>>,
}

impl<V: VertexTrait> ConsensusValueDependencies<V> {
    fn new() -> Self {
        ConsensusValueDependencies {
            cv_to_set: HashMap::new(),
            id_to_group: HashMap::new(),
        }
    }

    /// Adds a consensus value dependency.
    fn add(&mut self, c: V::Value, id: V::Id) {
        self.cv_to_set
            .entry(c.clone())
            .or_default()
            .push(id.clone());
        self.id_to_group.entry(id).or_default().insert(c);
    }

    /// Remove a consensus value from dependencies.
    /// Call when it's downloaded/synchronized.
    /// Returns vertices that were waiting on it.
    fn remove(&mut self, c: V::Value) -> Vec<V::Id> {
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

pub(crate) struct DagSynchronizerState<P>
where
    P: ProtocolState,
{
    consensus_value_deps: ConsensusValueDependencies<P::Vertex>,
    // Tracks which vertices are still waiting for its vertex dependencies to be downloaded.
    // Since a vertex can have multiple vertices depend on it, downloading single vertex
    // can "release" more than one new vertex to be requeued to the reactor.
    //TODO: Wrap the following with a struct that will keep the details hidden.
    vertex_dependants: HashMap<P::VId, Vec<P::VId>>,
    vertex_by_vid: HashMap<P::VId, P::Vertex>,
}

impl<P> DagSynchronizerState<P>
where
    P: ProtocolState,
{
    fn new() -> Self {
        DagSynchronizerState {
            consensus_value_deps: ConsensusValueDependencies::new(),
            vertex_dependants: HashMap::new(),
            vertex_by_vid: HashMap::new(),
        }
    }

    pub(crate) fn add_vertex(
        &mut self,
        sender: NodeId,
        v: P::Vertex,
        protocol_state: &mut P,
    ) -> Result<SynchronizerEffect<P::Vertex>, anyhow::Error> {
        match protocol_state.add_vertex(v.clone()) {
            Ok(AddVertexOk::MissingDependency(missing_vid)) => {
                self.add_vertex_dependency(missing_vid.clone(), v);
                Ok(SynchronizerEffect::RequestVertex(sender, missing_vid))
            }
            Ok(AddVertexOk::Success(_vid)) => {
                let vertices_with_completed_dependencies = self.complete_vertex_dependency(v.id());
                Ok(SynchronizerEffect::RequeueVertex(
                    vertices_with_completed_dependencies,
                ))
            }
            Err(error) => Err(anyhow::anyhow!("{:?}", error)),
        }
    }

    // Vertex `v` depends on the vertex with ID `v_id`
    fn add_vertex_dependency(&mut self, v_id: P::VId, v: P::Vertex) {
        let dependant_id = v.id();
        self.vertex_by_vid.entry(dependant_id.clone()).or_insert(v);
        self.vertex_dependants
            .entry(v_id)
            .or_insert_with(Vec::new)
            .push(dependant_id);
    }

    fn add_consensus_value_dependency(
        &mut self,
        c: <P::Vertex as VertexTrait>::Value,
        v: &P::Vertex,
    ) {
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
    fn complete_vertex_dependency(&mut self, v_id: P::VId) -> Vec<P::Vertex> {
        match self.vertex_dependants.remove(&v_id) {
            None => Vec::new(),
            Some(dependants) => self.get_vertices_by_id(dependants),
        }
    }

    /// Complete a consensus value dependency (called when that C is downloaded from another node).
    /// Returns list of vertices that were waiting on the completion of that consensus value.
    /// Vertices returned have all of its dependencies completed - i.e. are not waiting for anything
    /// else.
    fn complete_consensus_value_dependency(
        &mut self,
        c: <P::Vertex as VertexTrait>::Value,
    ) -> Vec<P::Vertex> {
        let dependants = self.consensus_value_deps.remove(c);
        if dependants.is_empty() {
            Vec::new()
        } else {
            self.get_vertices_by_id(dependants)
        }
    }

    /// Helper method for returning list of vertices by its ID.
    fn get_vertices_by_id(&mut self, vertex_ids: Vec<P::VId>) -> Vec<P::Vertex> {
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
        c: Vec<<P::Vertex as VertexTrait>::Value>,
        v: P::Vertex,
    ) -> SynchronizerEffect<P::Vertex> {
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
        missing_dependency: P::VId,
        new_vertex: P::Vertex,
    ) -> SynchronizerEffect<P::Vertex> {
        self.add_vertex_dependency(missing_dependency.clone(), new_vertex);
        SynchronizerEffect::RequestVertex(node, missing_dependency)
    }

    /// Must be called after consensus successfully handles the new vertex.
    /// That's b/c there might be other vertices that depend on this one and are waiting in a queue.
    fn on_vertex_synced(&mut self, v: P::VId) -> Vec<SynchronizerEffect<P::Vertex>> {
        let completed_dependencies = self.complete_vertex_dependency(v);
        completed_dependencies
            .into_iter()
            .map(|v| SynchronizerEffect::RequeueVertex(vec![v]))
            .collect()
    }

    fn on_consensus_value_synced(
        &mut self,
        c: <P::Vertex as VertexTrait>::Value,
    ) -> Vec<SynchronizerEffect<P::Vertex>> {
        let completed_dependencies = self.complete_consensus_value_dependency(c);
        completed_dependencies
            .into_iter()
            .map(|v| SynchronizerEffect::RequeueVertex(vec![v]))
            .collect()
    }
}
