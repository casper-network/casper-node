#[cfg(test)]
mod tests;
mod traits;

pub(crate) use traits::{DependencySpec, HandleNewItemResult, ItemWithId, NodeId, ProtocolState};

use std::collections::HashMap;

/// Data associated with an item in the queue that is still missing some dependencies
#[derive(Debug)]
struct QueueItem<N, C: ProtocolState> {
    original_sender: N,
    item: <C::DepSpec as DependencySpec>::Item,
    dependencies: C::DepSpec,
}

/// The main synchronizer struct - controlling which items have unresolved dependencies and
/// handling requests and responses regarding dependency resolution
#[derive(Debug, Default)]
pub(crate) struct Synchronizer<N: NodeId, C: ProtocolState> {
    dependency_queue: HashMap<<C::DepSpec as DependencySpec>::ItemId, QueueItem<N, C>>,
}

/// Messages that can be sent and handled by the synchronizer
#[derive(Debug)]
pub(crate) enum SynchronizerMessage<D: DependencySpec> {
    DependencyRequest(D::DependencyDescription),
    NewItem(D::ItemId, D::Item),
}

/// Struct aggregating the results of satisfying a new dependency for the items in the queue
#[derive(Debug)]
struct SatisfiedDependenciesResult<N: NodeId, C: ProtocolState> {
    messages: Vec<(N, SynchronizerMessage<C::DepSpec>)>,
    satisfied_deps: Vec<ItemWithId<C::DepSpec>>,
}

impl<N: NodeId, C: ProtocolState> Synchronizer<N, C> {
    /// Creates a new synchronizer
    pub(crate) fn new() -> Self {
        Self {
            dependency_queue: Default::default(),
        }
    }

    /// Handles a synchronizer message; returns new messages to be sent
    pub(crate) fn handle_message(
        &mut self,
        consensus: &mut C,
        sender: N,
        message: SynchronizerMessage<C::DepSpec>,
    ) -> Vec<(N, SynchronizerMessage<C::DepSpec>)> {
        match message {
            SynchronizerMessage::DependencyRequest(dependency_descr) => {
                self.handle_dependency_request(consensus, sender, dependency_descr)
            }
            SynchronizerMessage::NewItem(item_id, item) => {
                self.handle_new_item(consensus, sender, item_id, item)
            }
        }
    }

    fn handle_dependency_request(
        &mut self,
        consensus: &C,
        sender: N,
        dependency_descr: <C::DepSpec as DependencySpec>::DependencyDescription,
    ) -> Vec<(N, SynchronizerMessage<C::DepSpec>)> {
        if let Some(ItemWithId { item_id, item }) = consensus.get_dependency(&dependency_descr) {
            vec![(sender, SynchronizerMessage::NewItem(item_id, item))]
        } else {
            // TODO: handle the case when we don't have the requested dependency; currently we'll
            // just ignore the request
            // TBD: or maybe it shouldn't even happen, because we shouldn't be sending messages for
            // which we don't have all the dependencies?
            vec![]
        }
    }

    fn handle_new_item(
        &mut self,
        consensus: &mut C,
        sender: N,
        item_id: <C::DepSpec as DependencySpec>::ItemId,
        item: <C::DepSpec as DependencySpec>::Item,
    ) -> Vec<(N, SynchronizerMessage<C::DepSpec>)> {
        match consensus.handle_new_item(item_id.clone(), item.clone()) {
            HandleNewItemResult::Accepted => {
                let SatisfiedDependenciesResult {
                    mut messages,
                    satisfied_deps,
                } = self.collect_satisfied_dependencies(item_id);
                for ItemWithId { item_id, item } in satisfied_deps {
                    messages.extend(self.handle_new_item(consensus, sender.clone(), item_id, item));
                }
                messages
            }
            HandleNewItemResult::Invalid => vec![], // TODO: should we do something here?
            HandleNewItemResult::DependenciesMissing(mut deps) => {
                let next_dependency = deps.next_dependency();
                self.dependency_queue.insert(
                    item_id,
                    QueueItem {
                        original_sender: sender.clone(),
                        item,
                        dependencies: deps,
                    },
                );
                next_dependency
                    .into_iter()
                    .map(|dep| (sender.clone(), SynchronizerMessage::DependencyRequest(dep)))
                    .collect()
            }
        }
    }

    fn collect_satisfied_dependencies(
        &mut self,
        new_item_id: <C::DepSpec as DependencySpec>::ItemId,
    ) -> SatisfiedDependenciesResult<N, C> {
        let messages = self
            .dependency_queue
            .values_mut()
            .filter_map(|qitem| {
                if qitem.dependencies.resolve_dependency(new_item_id.clone()) {
                    qitem
                        .dependencies
                        .next_dependency()
                        .map(|dep| (qitem.original_sender.clone(), dep))
                } else {
                    None
                }
            })
            .map(|(sender, dep)| (sender, SynchronizerMessage::DependencyRequest(dep)))
            .collect::<Vec<_>>();
        let satisfied_ids = self
            .dependency_queue
            .iter()
            .filter(|(_, qitem)| qitem.dependencies.all_resolved())
            .map(|(item_id, _)| item_id.clone())
            .collect::<Vec<_>>();
        let satisfied_deps = satisfied_ids
            .into_iter()
            .filter_map(|item_id| {
                self.dependency_queue
                    .remove(&item_id)
                    .map(|qitem| ItemWithId {
                        item_id,
                        item: qitem.item,
                    })
            })
            .collect();
        SatisfiedDependenciesResult {
            messages,
            satisfied_deps,
        }
    }
}
