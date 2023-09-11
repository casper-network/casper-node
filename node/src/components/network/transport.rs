//! Low-level network transport configuration.
//!
//! The low-level transport is built on top of an existing TLS stream, handling all multiplexing. It
//! is based on a configuration of the Juliet protocol implemented in the `juliet` crate.

use juliet::{rpc::IncomingRequest, ChannelConfiguration};
use strum::EnumCount;

use super::Channel;

/// Creats a new RPC builder with the currently fixed Juliet configuration.
///
/// The resulting `RpcBuilder` can be reused for multiple connections.
pub(super) fn create_rpc_builder(
    maximum_message_size: u32,
    max_in_flight_demands: u16,
) -> juliet::rpc::RpcBuilder<{ Channel::COUNT }> {
    // Note: `maximum_message_size` is a bit misleading, since it is actually the maximum payload
    //       size. In the future, the chainspec setting should be overhauled and the
    //       one-size-fits-all limit replaced with a per-channel limit. Similarly,
    //       `max_in_flight_demands` should be tweaked on a per-channel basis.

    // Since we do not currently configure individual message size limits and make no distinction
    // between requests and responses, we simply set all limits to the maximum message size.
    let channel_cfg = ChannelConfiguration::new()
        .with_request_limit(max_in_flight_demands)
        .with_max_request_payload_size(maximum_message_size)
        .with_max_response_payload_size(maximum_message_size);

    let protocol = juliet::protocol::ProtocolBuilder::with_default_channel_config(channel_cfg);

    // TODO: Figure out a good value for buffer sizes.
    let io_core = juliet::io::IoCoreBuilder::with_default_buffer_size(
        protocol,
        max_in_flight_demands.min(20) as usize,
    );

    juliet::rpc::RpcBuilder::new(io_core)
}

/// Adapter for incoming Juliet requests.
///
/// At this time the node does not take full advantage of the Juliet RPC capabilities, relying on
/// its older message+ACK based model introduced with `muxink`. In this model, every message is only
/// acknowledged, with no request-response association being done. The ACK indicates that the peer
/// is free to send another message.
///
/// The [`Ticket`] type is used to track the processing of an incoming message or its resulting
/// operations; it should dropped once the resources for doing so have been spent, but no earlier.
///
/// Dropping it will cause an "ACK", which in the Juliet transport's case is an empty response, to
/// be sent. Cancellations or responses with actual payloads are not used at this time.
#[derive(Debug)]
pub(crate) struct Ticket(Option<Box<IncomingRequest>>);

impl Ticket {
    #[inline(always)]
    pub(super) fn from_rpc_request(incoming_request: IncomingRequest) -> Self {
        Ticket(Some(Box::new(incoming_request)))
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn create_dummy() -> Self {
        Ticket(None)
    }
}

impl Drop for Ticket {
    #[inline(always)]
    fn drop(&mut self) {
        // Currently, we simply send a request confirmation in the for of an `ACK`.
        if let Some(incoming_request) = self.0.take() {
            incoming_request.respond(None);
        }
    }
}
