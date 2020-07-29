use std::{fmt::Debug, hash::Hash};

pub(crate) trait VertexId: Debug + Clone + Hash + Eq + Ord {}
impl<T> VertexId for T where T: Debug + Clone + Hash + Eq + Ord {}

pub(crate) trait VertexTrait: Debug + Clone {
    type Id: VertexId;
    type Value: Debug + Clone + Hash + Eq;

    fn id(&self) -> Self::Id;

    fn value(&self) -> Option<&Self::Value>;
}

pub(crate) trait ProtocolState {
    type Error: Debug;
    type VId: VertexId;
    type Vertex: VertexTrait<Id = Self::VId>;

    fn missing_dependency(&self, v: &Self::Vertex) -> Option<Self::VId>;

    fn get_vertex(&self, v: Self::VId) -> Result<Option<Self::Vertex>, Self::Error>;
}
