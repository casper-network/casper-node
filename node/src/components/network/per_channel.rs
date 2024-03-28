//! `PerChannel` allows to hold some configuration for every channel. It has
//! convenience methods allowing to cover common operations.
//!
//! For example, `buffer_size: PerChannel<usize>` allows to associate a buffer
//! size of type `usize` to every channel.

use std::convert::Infallible;

use casper_types::bytesrepr::{self, FromBytes, ToBytes};
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use super::Channel;
use crate::utils::UnwrapInfallible;

/// Allows to hold some data for every channel used in the node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, DataSize, Serialize, Deserialize)]
pub struct PerChannel<T> {
    network: T,
    sync_data_request: T,
    sync_data_responses: T,
    data_requests: T,
    data_responses: T,
    consensus: T,
    bulk_gossip: T,
}

impl<T> PerChannel<T> {
    /// Returns data value stored for the given channel.
    #[inline(always)]
    pub const fn get(&self, channel: Channel) -> &T {
        match channel {
            Channel::Network => &self.network,
            Channel::SyncDataRequests => &self.sync_data_request,
            Channel::SyncDataResponses => &self.sync_data_responses,
            Channel::DataRequests => &self.data_requests,
            Channel::DataResponses => &self.data_responses,
            Channel::Consensus => &self.consensus,
            Channel::BulkGossip => &self.bulk_gossip,
        }
    }

    /// Creates a new `PerChannel` from the original one by applying the given function.
    pub fn map<U>(self, mut f: impl FnMut(Channel, T) -> U) -> PerChannel<U> {
        PerChannel {
            network: f(Channel::Network, self.network),
            sync_data_request: f(Channel::SyncDataRequests, self.sync_data_request),
            sync_data_responses: f(Channel::SyncDataResponses, self.sync_data_responses),
            data_requests: f(Channel::DataRequests, self.data_requests),
            data_responses: f(Channel::DataResponses, self.data_responses),
            consensus: f(Channel::Consensus, self.consensus),
            bulk_gossip: f(Channel::BulkGossip, self.bulk_gossip),
        }
    }

    /// Fill the fields for all the channels with a value generated from the given closure.
    #[inline(always)]
    pub fn init_with(mut initializer: impl FnMut(Channel) -> T) -> Self {
        Self::try_init_with::<Infallible>(|channel| Ok(initializer(channel))).unwrap_infallible()
    }

    /// Fill the fields for all the channels with a value generated from the given closure, reducing
    /// to a single result..
    pub fn try_init_with<E>(
        mut initializer: impl FnMut(Channel) -> Result<T, E>,
    ) -> Result<Self, E> {
        Ok(PerChannel {
            network: initializer(Channel::Network)?,
            sync_data_request: initializer(Channel::SyncDataRequests)?,
            sync_data_responses: initializer(Channel::SyncDataResponses)?,
            data_requests: initializer(Channel::DataRequests)?,
            data_responses: initializer(Channel::DataResponses)?,
            consensus: initializer(Channel::Consensus)?,
            bulk_gossip: initializer(Channel::BulkGossip)?,
        })
    }
}

impl<T> IntoIterator for PerChannel<T> {
    type Item = (Channel, T);

    type IntoIter = std::array::IntoIter<(Channel, T), 7>;

    fn into_iter(self) -> Self::IntoIter {
        let Self {
            network,
            sync_data_request,
            sync_data_responses,
            data_requests,
            data_responses,
            consensus,
            bulk_gossip,
        } = self;

        [
            (Channel::Network, network),
            (Channel::SyncDataRequests, sync_data_request),
            (Channel::SyncDataResponses, sync_data_responses),
            (Channel::DataRequests, data_requests),
            (Channel::DataResponses, data_responses),
            (Channel::Consensus, consensus),
            (Channel::BulkGossip, bulk_gossip),
        ]
        .into_iter()
    }
}

impl<T: ToBytes> ToBytes for PerChannel<T> {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        let Self {
            network,
            sync_data_request,
            sync_data_responses,
            data_requests,
            data_responses,
            consensus,
            bulk_gossip,
        } = self;

        buffer.extend(network.to_bytes()?);
        buffer.extend(sync_data_request.to_bytes()?);
        buffer.extend(sync_data_responses.to_bytes()?);
        buffer.extend(data_requests.to_bytes()?);
        buffer.extend(data_responses.to_bytes()?);
        buffer.extend(consensus.to_bytes()?);
        buffer.extend(bulk_gossip.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            network,
            sync_data_request,
            sync_data_responses,
            data_requests,
            data_responses,
            consensus,
            bulk_gossip,
        } = self;

        network.serialized_length()
            + sync_data_request.serialized_length()
            + sync_data_responses.serialized_length()
            + data_requests.serialized_length()
            + data_responses.serialized_length()
            + consensus.serialized_length()
            + bulk_gossip.serialized_length()
    }
}

impl<T: FromBytes> FromBytes for PerChannel<T> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (network, bytes) = FromBytes::from_bytes(bytes)?;
        let (sync_data_request, bytes) = FromBytes::from_bytes(bytes)?;
        let (sync_data_responses, bytes) = FromBytes::from_bytes(bytes)?;
        let (data_requests, bytes) = FromBytes::from_bytes(bytes)?;
        let (data_responses, bytes) = FromBytes::from_bytes(bytes)?;
        let (consensus, bytes) = FromBytes::from_bytes(bytes)?;
        let (bulk_gossip, bytes) = FromBytes::from_bytes(bytes)?;

        let config = Self {
            network,
            sync_data_request,
            sync_data_responses,
            data_requests,
            data_responses,
            consensus,
            bulk_gossip,
        };
        Ok((config, bytes))
    }
}
