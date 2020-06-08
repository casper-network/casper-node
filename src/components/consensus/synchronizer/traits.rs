use std::{fmt::Debug, hash::Hash};

pub(crate) trait Item: Clone + Debug {}
impl<T> Item for T where T: Clone + Debug {}

pub(crate) trait NodeId: Clone + Debug {}
impl<T> NodeId for T where T: Clone + Debug {}

pub(crate) trait Identifier: Clone + Debug + Eq + Ord + Hash {}
impl<T> Identifier for T where T: Clone + Debug + Eq + Ord + Hash {}

/// Trait for structures that can represent dependency requirements for an item.
/// A struct implementing this trait can yield all the dependencies in a sequence via
/// `next_dependency` and is able to track which dependencies are still not resolved.
pub(crate) trait DependencySpec: Clone + Debug {
    /// a type that describes a requirement for resolving dependencies of an item
    type DependencyDescription: Debug;
    /// an item that can satisfy a dependency
    type Item: Item;
    /// a small piece of data uniquely identifying an Item
    type ItemId: Identifier;

    /// Returns a next dependency to ask for
    fn next_dependency(&mut self) -> Option<Self::DependencyDescription>;

    /// called when `item_id` is received - returns whether the dependency was relevant (ie.,
    /// whether this was actually a dependency of this item)
    fn resolve_dependency(&mut self, item_id: Self::ItemId) -> bool;

    /// Returns whether all the dependencies are resolved
    fn all_resolved(&self) -> bool;
}

/// A result of calling `ConsensusProtocol::handle_new_item`
#[derive(Debug)]
pub(crate) enum HandleNewItemResult<D: DependencySpec> {
    Accepted,
    DependenciesMissing(D),
    Invalid,
}

/// A helper type to reduce type complexity in some method definitions
#[derive(Debug)]
pub(crate) struct ItemWithId<D: DependencySpec> {
    pub(crate) item_id: D::ItemId,
    pub(crate) item: D::Item,
}

/// Trait abstracting a consensus protocol interface with respect to dependency resolution. Some
/// messages of a consensus protocol can depend on other messages or other items - this interface
/// makes it possible for the protocol to signal missing dependencies and let the synchronizer
/// resolve them.
pub(crate) trait ProtocolState {
    type DepSpec: DependencySpec;

    /// To be called when the synchronizer is asked for a missing dependency by another node.
    fn get_dependency(
        &self,
        dep: &<Self::DepSpec as DependencySpec>::DependencyDescription,
    ) -> Option<ItemWithId<Self::DepSpec>>;

    /// To be called when the synchronizer receives a new item from another node.
    fn handle_new_item(
        &mut self,
        item_id: <Self::DepSpec as DependencySpec>::ItemId,
        item: <Self::DepSpec as DependencySpec>::Item,
    ) -> HandleNewItemResult<Self::DepSpec>;
}
