use std::{
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use serde::{de::DeserializeOwned, Serialize};

use super::Tag;

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Debug, DataSize)]
pub(crate) struct EmptyValidationMetadata;

impl Display for EmptyValidationMetadata {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(formatter, "no validation metadata")
    }
}

/// A trait which allows an implementing type to be used by a fetcher component.
pub(crate) trait FetchItem:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display + Eq
{
    /// The type of ID of the item.
    type Id: Clone + Eq + Hash + Serialize + DeserializeOwned + Send + Sync + Debug + Display;
    /// The error type returned when validating to get the ID of the item.
    type ValidationError: StdError + Debug + Display;
    /// The type of the metadata provided when validating the item.
    type ValidationMetadata: Eq + Clone + Serialize + Debug + DataSize + Send;

    /// The tag representing the type of the item.
    const TAG: Tag;

    /// The ID of the specific item.
    fn fetch_id(&self) -> Self::Id;

    /// Checks validity of the item, and returns an error if invalid.
    fn validate(&self, metadata: &Self::ValidationMetadata) -> Result<(), Self::ValidationError>;
}
