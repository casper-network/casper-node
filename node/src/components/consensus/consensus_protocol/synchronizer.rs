use std::collections::{hash_map::Entry, HashMap, HashSet};

use super::protocol_state::{ProtocolState, VertexTrait};
use crate::components::consensus::traits::NodeIdT;

/// Note that we might be requesting download of the duplicate element
/// (one that had requested for earlier) but with a different node.
/// The assumption is that a downloading layer will collect different node IDs as alternative
/// sources and use different address in the case of download failures.
#[derive(Debug)]
pub(crate) enum SynchronizerEffect<I, V: VertexTrait> {
    /// Effect for the reactor to download missing vertex.
    RequestVertex(I, V::Id),
    /// Effect for the reactor to download missing consensus values (a deploy for example).
    RequestConsensusValue(I, V::Value),
    /// Effect for the reactor to requeue a vertex once its dependencies are downloaded.
    RequeueVertex(I, V),
    /// Means that the vertex has all dependencies (vertices and consensus values)
    /// successfully added to the state.
    Ready(V),
}

/// Structure that tracks which vertices wait for what consensus value dependencies.
#[derive(Debug)]
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
    fn remove(&mut self, c: &V::Value) -> Vec<V::Id> {
        // Get list of vertices that are dependent for the consensus value.
        match self.cv_to_set.remove(c) {
            None => Vec::new(),
            Some(dependent_vertices) => {
                // Remove the consensus value from the set of values each vertex is waiting for.
                dependent_vertices
                    .into_iter()
                    .filter(|vertex| {
                        if let Entry::Occupied(mut consensus_values) =
                            self.id_to_group.entry(vertex.clone())
                        {
                            consensus_values.get_mut().remove(c);
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

#[derive(Debug)]
pub(crate) struct DagSynchronizerState<I, P>
where
    P: ProtocolState,
{
    consensus_value_deps: ConsensusValueDependencies<P::Vertex>,
    // Tracks which vertices are still waiting for its vertex dependencies to be downloaded.
    // Since a vertex can have multiple vertices depend on it, downloading single vertex
    // can "release" more than one new vertex to be requeued to the reactor.
    //TODO: Wrap the following with a struct that will keep the details hidden.
    vertex_dependants: HashMap<P::VId, Vec<P::VId>>,
    vertex_by_vid: HashMap<P::VId, (I, P::Vertex)>,
}

impl<I, P> DagSynchronizerState<I, P>
where
    I: NodeIdT,
    P: ProtocolState,
{
    pub(crate) fn new() -> Self {
        DagSynchronizerState {
            consensus_value_deps: ConsensusValueDependencies::new(),
            vertex_dependants: HashMap::new(),
            vertex_by_vid: HashMap::new(),
        }
    }

    /// Synchronizes vertex `v`.
    ///
    /// If protocol state is missing vertex dependencies of `v` returns vertex request effect.
    /// If protocol state is sync'd, returns request to synchronize consensus value.
    /// If all dependencies are satisfied, returns `Ready(v)` signaling that `v`
    /// can be safely added to the protocol state and `RequeueVertex` effects for vertices
    /// that were depending on `v`.
    pub(crate) fn synchronize_vertex(
        &mut self,
        sender: I,
        v: P::Vertex,
        protocol_state: &P,
    ) -> Vec<SynchronizerEffect<I, P::Vertex>> {
        if let Some(missing_vid) = protocol_state.missing_dependency(&v) {
            // If there are missing vertex dependencies, sync those first.
            vec![self.sync_vertex(sender, missing_vid, v.clone())]
        } else {
            // Once all vertex dependencies are satisfied, we need to make sure that
            // we also have all the block's deploys.
            match self.sync_consensus_values(sender, &v) {
                Some(eff) => vec![eff],
                None => {
                    // Downloading vertex `v` may resolve the dependency requests for other
                    // vertices. If it's a vote that has no consensus values it
                    // can be added to the state.
                    self.on_vertex_fully_synced(v)
                }
            }
        }
    }

    // Vertex `v` depends on the vertex with ID `v_id`
    fn add_vertex_dependency(&mut self, v_id: P::VId, sender: I, v: P::Vertex) {
        let dependant_id = v.id();
        self.vertex_by_vid
            .entry(dependant_id.clone())
            .or_insert((sender, v));
        self.vertex_dependants
            .entry(v_id)
            .or_insert_with(Vec::new)
            .push(dependant_id);
    }

    fn add_consensus_value_dependency(&mut self, c: P::Value, sender: I, v: &P::Vertex) {
        let dependant_id = v.id();
        self.vertex_by_vid
            .entry(dependant_id.clone())
            .or_insert_with(|| (sender, v.clone()));
        self.consensus_value_deps.add(c, dependant_id)
    }

    /// Complete a vertex dependency (called when that vertex is downloaded from another node and
    /// persisted). Returns list of vertices that were waiting on that vertex dependency.
    /// Vertices returned have all of its dependencies completed - i.e. are not waiting for
    /// anything else.
    fn complete_vertex_dependency(&mut self, v_id: P::VId) -> Vec<(I, P::Vertex)> {
        match self.vertex_dependants.remove(&v_id) {
            None => Vec::new(),
            Some(dependants) => self.get_vertices_by_id(dependants),
        }
    }

    /// Removes a consensus value dependency (called when that C is downloaded from another node).
    /// Returns list of vertices that were waiting on the completion of that consensus value.
    /// Vertices returned have all of its dependencies completed - i.e. are not waiting for anything
    /// else.
    fn remove_consensus_value_dependency(&mut self, c: &P::Value) -> Vec<(I, P::Vertex)> {
        let dependants = self.consensus_value_deps.remove(c);
        if dependants.is_empty() {
            Vec::new()
        } else {
            self.get_vertices_by_id(dependants)
        }
    }

    /// Helper method for returning list of vertices by its ID.
    fn get_vertices_by_id(&mut self, vertex_ids: Vec<P::VId>) -> Vec<(I, P::Vertex)> {
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
        sender: I,
        v: &P::Vertex,
    ) -> Option<SynchronizerEffect<I, P::Vertex>> {
        v.value().map(|cv| {
            self.add_consensus_value_dependency(cv.clone(), sender.clone(), v);
            SynchronizerEffect::RequestConsensusValue(sender, cv.clone())
        })
    }

    /// Synchronizes the dependency (single) of a newly received vertex.
    /// In practice, this method will produce an effect that will be passed on to the reactor for
    /// handling. Node passed in is the one that proposed the original vertex. It should also
    /// have the missing dependency.
    fn sync_vertex(
        &mut self,
        node: I,
        missing_dependency: P::VId,
        new_vertex: P::Vertex,
    ) -> SynchronizerEffect<I, P::Vertex> {
        self.add_vertex_dependency(missing_dependency.clone(), node.clone(), new_vertex);
        SynchronizerEffect::RequestVertex(node, missing_dependency)
    }

    /// Vertex `v` has been fully sync'd – both protocol state dependencies
    /// and consensus values have been downloaded.
    /// Returns vector of synchronizer effects to be handled.
    pub(crate) fn on_vertex_fully_synced(
        &mut self,
        v: P::Vertex,
    ) -> Vec<SynchronizerEffect<I, P::Vertex>> {
        let v_id = v.id();
        let mut effects = vec![SynchronizerEffect::Ready(v)];
        let completed_dependencies = self.complete_vertex_dependency(v_id);
        effects.extend(
            completed_dependencies
                .into_iter()
                .map(|(s, v)| SynchronizerEffect::RequeueVertex(s, v)),
        );
        effects
    }

    /// Marks `c` consensus value as downloaded.
    /// Returns vertices that were dependant on it.
    /// NOTE: Must be called only after all vertex dependencies are downloaded.
    pub(crate) fn on_consensus_value_synced(
        &mut self,
        c: &P::Value,
    ) -> Vec<SynchronizerEffect<I, P::Vertex>> {
        let completed_dependencies = self.remove_consensus_value_dependency(c);
        completed_dependencies
            .into_iter()
            // Because we sync consensus value dependencies only when we have sync'd
            // all vertex dependencies, we can now consider `v` to have all dependencies resolved.
            .flat_map(|(_, v)| self.on_vertex_fully_synced(v))
            .collect()
    }

    /// Drops all vertices depending directly or indirectly on `vid` and returns all senders.
    pub(crate) fn on_vertex_invalid(&mut self, vid: P::VId) -> HashSet<I> {
        let mut faulty_senders = HashSet::new();
        let mut invalid_ids = vec![vid];
        // All vertices waiting for invalid vertices are invalid as well.
        while let Some(vid) = invalid_ids.pop() {
            faulty_senders.extend(self.vertex_by_vid.remove(&vid).map(|(s, _)| s));
            invalid_ids.extend(self.vertex_dependants.remove(&vid).into_iter().flatten());
            for dependants in self.vertex_dependants.values_mut() {
                dependants.retain(|dvid| *dvid != vid);
            }
        }
        self.vertex_dependants.retain(|_, dep| !dep.is_empty());
        faulty_senders
    }

    /// Drops all vertices depending directly or indirectly on value `c` and returns all senders.
    pub(crate) fn on_consensus_value_invalid(&mut self, c: &P::Value) -> HashSet<I> {
        // All vertices waiting for this dependency are invalid.
        let (faulty_senders, invalid_ids): (HashSet<I>, HashSet<P::VId>) = self
            .remove_consensus_value_dependency(c)
            .into_iter()
            .map(|(sender, v)| (sender, v.id()))
            .unzip();
        // And all vertices waiting for invalid vertices are invalid as well.
        invalid_ids
            .into_iter()
            .flat_map(|vid| self.on_vertex_invalid(vid))
            .chain(faulty_senders)
            .collect()
    }
}
