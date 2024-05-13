//! Failpoint stubs.
//!
//! This module stubs out enough of the failpoint API to work if the feature is disabled, but never
//! activates them.

use std::{
    fmt::{self, Display, Formatter},
    marker::PhantomData,
    str::FromStr,
};

use datasize::DataSize;
use serde::Serialize;
use thiserror::Error;

/// A dummy failpoint.
#[derive(DataSize, Debug)]
pub(crate) struct Failpoint<T> {
    _phantom: PhantomData<T>,
}

impl<T> Failpoint<T> {
    /// Creates a new failpoint with a given key.
    #[inline(always)]
    pub(crate) fn new(_key: &'static str) -> Self {
        Failpoint {
            _phantom: PhantomData,
        }
    }

    /// Creates a new failpoint with a given key and optional subkey.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn new_with_subkey<S: ToString>(_key: &'static str, _subkey: S) -> Self {
        Failpoint {
            _phantom: PhantomData,
        }
    }

    /// Ignores the failpoint activation.
    #[inline(always)]
    pub(crate) fn update_from(&mut self, _activation: &FailpointActivation) {}

    /// Returns `None`.
    #[inline(always)]
    pub(crate) fn fire<R>(&mut self, _rng: &mut R) -> Option<&T> {
        None
    }
}

/// A parsed failpoint activation.
#[derive(Clone, DataSize, Debug, PartialEq, Serialize)]
pub(crate) struct FailpointActivation;

impl FailpointActivation {
    #[allow(dead_code)]
    pub(crate) fn new<S: ToString>(_key: S) -> FailpointActivation {
        FailpointActivation
    }

    pub(crate) fn key(&self) -> &str {
        ""
    }
}

impl Display for FailpointActivation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("(no failpoint support)")
    }
}

/// Error parsing a failpoint activation.
#[derive(Debug, Error)]
#[error("no failpoint support enabled")]
pub(crate) struct ParseError;

impl FromStr for FailpointActivation {
    type Err = ParseError;

    #[inline(always)]
    fn from_str(_raw: &str) -> Result<Self, Self::Err> {
        Err(ParseError)
    }
}
