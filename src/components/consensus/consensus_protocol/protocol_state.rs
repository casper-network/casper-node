use std::fmt::Debug;

pub(crate) trait VertexId {}

pub(crate) trait Vertex<C, Id> {
    fn id(&self) -> Id;

    fn values(&self) -> Vec<C>;
}

pub(crate) trait ProtocolState<VertexId, Vertex> {
    type Error: Debug;

    fn add_vertex(&mut self, v: Vertex) -> Result<Option<VertexId>, Self::Error>;

    fn get_vertex(&self, v: VertexId) -> Result<Option<Vertex>, Self::Error>;
}
