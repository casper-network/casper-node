//! # Highway
//!
//! The core logic of Casper' Highway consensus protocol.
//!
//! At the center of Highway are:
//! * the _protocol state_, a grow-only data structure which can be considered a directed acyclic
//!   graph (DAG), and needs to be continually synchronized among the participating nodes,
//! * rules for the active participants — the _validators_ — to create and add new vertices, and
//! * a finality detector that provides a criterion to consider a block "finalized". Finalized
//!   blocks are guaranteed to remain finalized as the DAG grows further, unless too many validators
//!   are malicious.
//!
//! It's not a complete protocol. To implement permissioned consensus, several components must be
//! added:
//! * Networking, serialization and cryptographic primitives for signing and hashing.
//! * A _synchronizer_ that exchanges messages with other participating nodes to exchange their DAG
//!   vertices and ensure that each vertex becomes eventually known to every node.
//! * Semantics for the consensus values, which can e.g. represent token transfers, or programs to
//!   be executed in a virtual machine for a smart contract platform.
//! * Signing of finalized blocks, as a finality proof to third parties/clients.
//!
//! Note that consensus values should be small. If they represent a lot of data, e.g. lists of
//! complex transactions, they should not be passed into `highway_core` directly. Instead, the
//! consensus value should be the list's hash.
//!
//! Permissioned consensus protocols can also be used in a _permissionless_ Proof-of-Stake context,
//! or with some other governance system that can add and remove validators, by starting a new
//! protocol instance whenever the set of validators changes.

// This needs to come before the other modules, so the macros are available everywhere.
#[cfg(test)]
#[macro_use]
mod test_macros;

pub(crate) mod active_validator;
pub(crate) mod finality_detector;
pub(crate) mod highway;
pub(crate) mod state;
pub(super) mod synchronizer;
pub(crate) mod validators;

mod endorsement;
mod evidence;
#[cfg(test)]
pub(crate) mod highway_testing;

pub(crate) use state::{State, Weight};

// Enables the endorsement mechanism.
const ENABLE_ENDORSEMENTS: bool = false;
