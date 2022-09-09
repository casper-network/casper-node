use datasize::DataSize;

use super::FetchResponder;
use crate::types::FetcherItem;

#[derive(Debug, DataSize)]
pub(crate) struct ItemHandle<T>
where
    T: FetcherItem,
{
    #[data_size(skip)]
    validation_metadata: T::ValidationMetadata,
    responders: Vec<FetchResponder<T>>,
}

impl<T: FetcherItem> ItemHandle<T> {
    pub(super) fn new(
        validation_metadata: T::ValidationMetadata,
        responder: FetchResponder<T>,
    ) -> Self {
        Self {
            validation_metadata,
            responders: vec![responder],
        }
    }

    pub(super) fn validation_metadata(&self) -> &T::ValidationMetadata {
        &self.validation_metadata
    }

    pub(super) fn push_responder(&mut self, responder: FetchResponder<T>) {
        self.responders.push(responder)
    }

    pub(super) fn pop_front_responder(&mut self) -> Option<FetchResponder<T>> {
        if self.responders.is_empty() {
            return None;
        }
        Some(self.responders.remove(0))
    }

    pub(super) fn take_responders(self) -> Vec<FetchResponder<T>> {
        self.responders
    }

    pub(super) fn has_no_responders(&self) -> bool {
        self.responders.is_empty()
    }
}
