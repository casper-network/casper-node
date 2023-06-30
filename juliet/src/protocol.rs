//! Protocol parsing state machine.
//!
//! The [`JulietProtocol`] type is designed to encapsulate the entire juliet protocol without any
//! dependencies on IO facilities; it can thus be dropped into almost any environment (`std::io`,
//! various `async` runtimes, etc.) with no changes.
//!
//! ## Usage
//!
//! TBW

mod multiframe;
mod outgoing_message;

use std::{collections::HashSet, num::NonZeroU32};

use bytes::{Buf, Bytes, BytesMut};
use thiserror::Error;

use self::{multiframe::MultiframeReceiver, outgoing_message::OutgoingMessage};
use crate::{
    header::{self, ErrorKind, Header, Kind},
    try_outcome, ChannelConfiguration, ChannelId, Id,
    Outcome::{self, Fatal, Incomplete, Success},
};

/// A channel ID to fill in when the channel is actually or not relevant unknown.
///
/// Note that this is not a reserved channel, just a default chosen -- it may clash with an
/// actually active channel.
const UNKNOWN_CHANNEL: ChannelId = ChannelId::new(0);

/// An ID to fill in when the ID should not matter.
///
/// Note a reserved id, it may clash with existing ones.
const UNKNOWN_ID: Id = Id::new(0);

/// A parser/state machine that processes an incoming stream and is able to construct messages to
/// send out.
///
/// This type does not handle IO, rather it expects a growing [`BytesMut`] buffer to be passed in,
/// containing incoming data. `N` denotes the number of valid channels, which should be fixed and
/// agreed upon by both peers prior to initialization.
///
/// Various methods for creating produce [`OutgoingMessage`] values, these should be converted into
/// frames (via [`OutgoingMessage::frames()`]) and the resulting frames sent to the peer.
#[derive(Debug)]
pub struct JulietProtocol<const N: usize> {
    /// Bi-directional channels.
    channels: [Channel; N],
    /// The maximum size for a single frame.
    max_frame_size: u32,
}

/// A builder for a [`JulietProtocol`] instance.
///
/// Created using [`JulietProtocol::builder`].
///
/// # Note
///
/// Typically a single instance of the [`ProtocolBuilder`] can be kept around in an application
/// handling multiple connections, as its `build()` method can be reused for every new connection
/// instance.
pub struct ProtocolBuilder<const N: usize> {
    /// Configuration for every channel.
    channel_config: [ChannelConfiguration; N],
    /// Maximum frame size.
    max_frame_size: u32,
}

impl<const N: usize> ProtocolBuilder<N> {
    /// Update the channel configuration for a given channel.
    pub fn channel_config(mut self, channel: ChannelId, config: ChannelConfiguration) -> Self {
        self.channel_config[channel.get() as usize] = config;
        self
    }

    /// Constructs a new protocol instance from the given builder.
    pub fn build(&self) -> JulietProtocol<N> {
        let channels: [Channel; N] =
            array_init::map_array_init(&self.channel_config, |cfg| Channel::new(*cfg));

        JulietProtocol {
            channels,
            max_frame_size: self.max_frame_size,
        }
    }
}

/// Per-channel data.
///
/// Used internally by the protocol to keep track. This data structure closely tracks the
/// information specified in the juliet RFC.
#[derive(Debug)]
struct Channel {
    /// A set of request IDs from requests received that have not been answered with a response or
    /// cancellation yet.
    incoming_requests: HashSet<Id>,
    /// A set of request IDs for requests made for which no response or cancellation has been
    /// received yet.
    outgoing_requests: HashSet<Id>,
    /// The multiframe receiver state machine.
    ///
    /// Every channel allows for at most one multi-frame message to be in progress at the same time.
    current_multiframe_receive: MultiframeReceiver,
    /// Number of requests received minus number of cancellations received.
    ///
    /// Capped at the request limit.
    cancellation_allowance: u16,
    /// Protocol-specific configuration values.
    config: ChannelConfiguration,
    /// The last request ID generated.
    prev_request_id: u16,
}

impl Channel {
    /// Creates a new channel, based on the given configuration.
    #[inline(always)]
    fn new(config: ChannelConfiguration) -> Self {
        Channel {
            incoming_requests: Default::default(),
            outgoing_requests: Default::default(),
            current_multiframe_receive: MultiframeReceiver::default(),
            cancellation_allowance: 0,
            config,
            prev_request_id: 0,
        }
    }

    /// Returns whether or not the peer has exhausted the number of requests allowed.
    ///
    /// Depending on the size of the payload an [`OutgoingMessage`] may span multiple frames. On a
    /// single channel, only one multi-frame message may be in the process of sending at a time,
    /// thus it is not permissable to begin sending frames of a different multi-frame message before
    /// the send of a previous one has been completed.
    ///
    /// Additional single-frame messages can be interspersed in between at will.
    ///
    /// [`JulietProtocol`] does not track whether or not a multi-channel message is in-flight; it is
    /// up to the caller to ensure no second multi-frame message commences sending before the first
    /// one completes.
    ///
    /// This problem can be avoided in its entirety if all frames of all messages created on a
    /// single channel are sent in the order they are created.
    ///
    /// Additionally frames of a single message may also not be reordered.
    #[inline]
    pub fn is_at_max_incoming_requests(&self) -> bool {
        self.incoming_requests.len() == self.config.request_limit as usize
    }

    /// Increments the cancellation allowance if possible.
    ///
    /// This method should be called everytime a valid request is received.
    #[inline]
    fn increment_cancellation_allowance(&mut self) {
        if self.cancellation_allowance < self.config.request_limit {
            self.cancellation_allowance += 1;
        }
    }

    /// Generates an unused ID for an outgoing request on this channel.
    ///
    /// Returns `None` if the entire ID space has been exhausted. Note that this should never
    /// occur under reasonable conditions, as the request limit should be less than [`u16::MAX`].
    #[inline]
    fn generate_request_id(&mut self) -> Option<Id> {
        if self.outgoing_requests.len() == u16::MAX as usize {
            // We've exhausted the entire ID space.
            return None;
        }

        let mut candidate = Id(self.prev_request_id.wrapping_add(1));
        while self.outgoing_requests.contains(&candidate) {
            candidate = Id(candidate.0.wrapping_add(1));
        }

        self.prev_request_id = candidate.0;

        Some(candidate)
    }

    /// Returns whether or not it is permissible to send another request on given channel.
    #[inline]
    pub fn allowed_to_send_request(&self) -> bool {
        self.outgoing_requests.len() < self.config.request_limit as usize
    }
}

/// A successful read from the peer.
#[must_use]
pub enum CompletedRead {
    /// An error has been received.
    ///
    /// The connection on our end should be closed, the peer will do the same.
    ErrorReceived {
        /// The error header.
        header: Header,
        /// The error data (only with [`ErrorKind::Other`]).
        data: Option<[u8; 4]>,
    },
    /// A new request has been received.
    NewRequest {
        /// The ID of the request.
        id: Id,
        /// Request payload.
        payload: Option<Bytes>,
    },
    /// A response to one of our requests has been received.
    ReceivedResponse {
        /// The ID of the request received.
        id: Id,
        /// The response payload.
        payload: Option<Bytes>,
    },
    /// A request was cancelled by the peer.
    RequestCancellation {
        /// ID of the request to be cancelled.
        id: Id,
    },
    /// A response was cancelled by the peer.
    ResponseCancellation {
        /// The ID of the response to be cancelled.
        id: Id,
    },
}

/// The caller of the this crate has violated the protocol.
///
/// A correct implementation of a client should never encounter this, thus simply unwrapping every
/// instance of this as part of a `Result<_, LocalProtocolViolation>` is usually a valid choice.
#[derive(Copy, Clone, Debug, Error)]
pub enum LocalProtocolViolation {
    /// A request was not sent because doing so would exceed the request limit on channel.
    ///
    /// Wait for addtional requests to be cancelled or answered. Calling
    /// [`JulietProtocol::allowed_to_send_request()`] before hand is recommended.
    #[error("sending would exceed request limit")]
    WouldExceedRequestLimit,
    /// The channel given does not exist.
    ///
    /// The given [`ChannelId`] exceeds `N` of [`JulietProtocol<N>`].
    #[error("invalid channel")]
    InvalidChannel(ChannelId),
    /// The given payload exceeds the configured limit.
    #[error("payload exceeds configured limit")]
    PayloadExceedsLimit,
}

impl<const N: usize> JulietProtocol<N> {
    /// Creates a new juliet protocol builder instance.
    ///
    /// All channels will initially be set to upload limits using `default_max_payload`.
    ///
    /// # Panics
    ///
    /// Will panic if `max_frame_size` is too small to hold header and payload length encoded, i.e.
    /// < 9 bytes.
    #[inline]
    pub fn builder(config: ChannelConfiguration) -> ProtocolBuilder<N> {
        ProtocolBuilder {
            channel_config: [config; N],
            max_frame_size: 1024,
        }
    }

    /// Looks up a given channel by ID.
    ///
    /// Returns a `LocalProtocolViolation` if called with non-existant channel.
    #[inline(always)]
    fn lookup_channel(&self, channel: ChannelId) -> Result<&Channel, LocalProtocolViolation> {
        if channel.0 as usize >= N {
            Err(LocalProtocolViolation::InvalidChannel(channel))
        } else {
            Ok(&self.channels[channel.0 as usize])
        }
    }

    /// Looks up a given channel by ID, mutably.
    ///
    /// Returns a `LocalProtocolViolation` if called with non-existant channel.
    #[inline(always)]
    fn lookup_channel_mut(
        &mut self,
        channel: ChannelId,
    ) -> Result<&mut Channel, LocalProtocolViolation> {
        if channel.0 as usize >= N {
            Err(LocalProtocolViolation::InvalidChannel(channel))
        } else {
            Ok(&mut self.channels[channel.0 as usize])
        }
    }

    /// Returns whether or not it is permissible to send another request on given channel.
    #[inline]
    pub fn allowed_to_send_request(
        &self,
        channel: ChannelId,
    ) -> Result<bool, LocalProtocolViolation> {
        self.lookup_channel(channel)
            .map(Channel::allowed_to_send_request)
    }

    /// Creates a new request to be sent.
    ///
    /// The outgoing request message's ID will be recorded in the outgoing set, for this reason a
    /// caller must send the returned outgoing message or it will be considered in-flight
    /// perpetually, unless explicitly cancelled.
    ///
    /// The resulting messages may be multi-frame messages, see
    /// [`OutgoingMessage::is_multi_frame()`]) for details.
    ///
    /// # Local protocol violations
    ///
    /// Will return a [`LocalProtocolViolation`] when attempting to send on an invalid channel, the
    /// payload exceeds the configured maximum for the channel, or if the request rate limit has
    /// been exceeded. Call [`JulietProtocol::allowed_to_send_request`] before calling
    /// `create_request` to avoid this.
    pub fn create_request(
        &mut self,
        channel: ChannelId,
        payload: Option<Bytes>,
    ) -> Result<OutgoingMessage, LocalProtocolViolation> {
        let chan = self.lookup_channel_mut(channel)?;

        if let Some(ref payload) = payload {
            if payload.len() > chan.config.max_request_payload_size as usize {
                return Err(LocalProtocolViolation::PayloadExceedsLimit);
            }
        }

        if !chan.allowed_to_send_request() {
            return Err(LocalProtocolViolation::WouldExceedRequestLimit);
        }

        // The `unwrap_or_default` below should never be triggered, as long as `u16::MAX` or less
        // requests are currently in flight, which is always the case.
        let id = chan.generate_request_id().unwrap_or(Id(0));

        // Record the outgoing request for later.
        chan.outgoing_requests.insert(id);

        if let Some(payload) = payload {
            let header = Header::new(header::Kind::RequestPl, channel, id);
            Ok(OutgoingMessage::new(header, Some(payload)))
        } else {
            let header = Header::new(header::Kind::Request, channel, id);
            Ok(OutgoingMessage::new(header, None))
        }
    }

    /// Creates a new response to be sent.
    ///
    /// If the ID was not in the outgoing set, it is assumed to have been cancelled earlier, thus no
    /// response should be sent and `None` is returned by this method.
    ///
    /// Calling this method frees up a request ID, thus giving the remote peer permission to make
    /// additional requests. While a legitimate peer will not know about the free ID until is has
    /// received either a response or cancellation sent from the local end, an hostile peer could
    /// attempt to spam if it knew the ID was going to be available quickly. For this reason, it is
    /// recommended to not create responses too eagerly, rather only one at a time after the
    /// previous response has finished sending.
    ///
    /// # Local protocol violations
    ///
    /// Will return a [`LocalProtocolViolation`] when attempting to send on an invalid channel or
    /// the payload exceeds the configured maximum for the channel.
    pub fn create_response(
        &mut self,
        channel: ChannelId,
        id: Id,
        payload: Option<Bytes>,
    ) -> Result<Option<OutgoingMessage>, LocalProtocolViolation> {
        let chan = self.lookup_channel_mut(channel)?;

        if !chan.incoming_requests.remove(&id) {
            // The request has been cancelled, no need to send a response.
            return Ok(None);
        }

        if let Some(ref payload) = payload {
            if payload.len() > chan.config.max_response_payload_size as usize {
                return Err(LocalProtocolViolation::PayloadExceedsLimit);
            }
        }

        if let Some(payload) = payload {
            let header = Header::new(header::Kind::ResponsePl, channel, id);
            Ok(Some(OutgoingMessage::new(header, Some(payload))))
        } else {
            let header = Header::new(header::Kind::Response, channel, id);
            Ok(Some(OutgoingMessage::new(header, None)))
        }
    }

    /// Creates a cancellation for an outgoing request.
    ///
    /// If the ID is not in the outgoing set, due to already being responsed to or cancelled, `None`
    /// will be returned.
    ///
    /// If the caller does not track the use of IDs separately to the [`JulietProtocol`] structure,
    /// it is possible to cancel an ID that has already been reused. To avoid this, a caller should
    /// take measures to ensure that only response or cancellation is ever sent for a given request.
    ///
    /// # Local protocol violations
    ///
    /// Will return a [`LocalProtocolViolation`] when attempting to send on an invalid channel.
    pub fn cancel_request(
        &mut self,
        channel: ChannelId,
        id: Id,
    ) -> Result<Option<OutgoingMessage>, LocalProtocolViolation> {
        let chan = self.lookup_channel_mut(channel)?;

        if !chan.outgoing_requests.remove(&id) {
            // The request has been cancelled, no need to send a response. This also prevents us
            // from ever violating the cancellation limit by accident, if all requests are sent
            // properly.
            return Ok(None);
        }

        let header = Header::new(header::Kind::CancelReq, channel, id);
        Ok(Some(OutgoingMessage::new(header, None)))
    }

    /// Creates a cancellation of an incoming request.
    ///
    /// Incoming request cancellations are used to indicate that the local peer cannot or will not
    /// respond to a given request. Since only either a response or a cancellation can be sent for
    /// any given request, this function will return `None` if the given ID cannot be found in the
    /// outbound set.
    ///
    /// # Local protocol violations
    ///
    /// Will return a [`LocalProtocolViolation`] when attempting to send on an invalid channel.
    pub fn cancel_response(
        &mut self,
        channel: ChannelId,
        id: Id,
    ) -> Result<Option<OutgoingMessage>, LocalProtocolViolation> {
        let chan = self.lookup_channel_mut(channel)?;

        if !chan.incoming_requests.remove(&id) {
            // The request has been cancelled, no need to send a response.
            return Ok(None);
        }

        let header = Header::new(header::Kind::CancelReq, channel, id);
        Ok(Some(OutgoingMessage::new(header, None)))
    }

    /// Creates an error message with type [`ErrorKind::Other`].
    ///
    /// # Local protocol violations
    ///
    /// Will return a [`LocalProtocolViolation`] when attempting to send on an invalid channel.
    pub fn custom_error(&mut self, channel: ChannelId, id: Id, payload: Bytes) -> OutgoingMessage {
        let header = Header::new_error(header::ErrorKind::Other, channel, id);
        OutgoingMessage::new(header, Some(payload))
    }

    /// Processes incoming data from a buffer.
    ///
    /// This is the main ingress function of [`JulietProtocol`]. `buffer` should continuously be
    /// appended with all incoming data; the [`Outcome`] returned indicates when the function should
    /// be called next:
    ///
    /// * [`Outcome::Success`] indicates `process_incoming` should be called again as early as
    ///   possible, since additional messages may already be contained in `buffer`.
    /// * [`Outcome::Incomplete(n)`] tells the caller to not call `process_incoming` again before at
    ///   least `n` additional bytes have been added to bufer.
    /// * [`Outcome::Fatal`] indicates that the remote peer violated the protocol, the returned
    ///   [`Header`] should be attempted to be sent to the peer before the connection is being
    ///   closed.
    ///
    /// This method transparently handles multi-frame sends, any incomplete messages will be
    /// buffered internally until they are complete.
    ///
    /// Any successful frame read will cause `buffer` to be advanced by the length of the frame,
    /// thus eventually freeing the data if not held elsewhere.
    pub fn process_incoming(
        &mut self,
        mut buffer: BytesMut,
    ) -> Outcome<CompletedRead, OutgoingMessage> {
        // First, attempt to complete a frame.
        loop {
            // We do not have enough data to extract a header, indicate and return.
            if buffer.len() < Header::SIZE {
                return Incomplete(NonZeroU32::new((Header::SIZE - buffer.len()) as u32).unwrap());
            }

            let header_raw: [u8; Header::SIZE] = buffer[0..Header::SIZE].try_into().unwrap();
            let header = match Header::parse(header_raw) {
                Some(header) => header,
                None => {
                    // The header was invalid, return an error.
                    return Fatal(OutgoingMessage::new(
                        Header::new_error(ErrorKind::InvalidHeader, UNKNOWN_CHANNEL, UNKNOWN_ID),
                        None,
                    ));
                }
            };

            // We have a valid header, check if it is an error.
            if header.is_error() {
                match header.error_kind() {
                    ErrorKind::Other => {
                        // `Other` allows for adding error data, which is fixed at 4 bytes.
                        let expected_total_length = buffer.len() + Header::SIZE + 4;

                        if buffer.len() < expected_total_length {
                            return Outcome::incomplete(expected_total_length - buffer.len());
                        }

                        let data = buffer[4..8]
                            .try_into()
                            .expect("did not expect previously bounds checked buffer read to fail");

                        return Success(CompletedRead::ErrorReceived {
                            header,
                            data: Some(data),
                        });
                    }
                    _ => {
                        return Success(CompletedRead::ErrorReceived { header, data: None });
                    }
                }
            }

            // At this point we are guaranteed a valid non-error frame, verify its channel.
            let channel = match self.channels.get_mut(header.channel().get() as usize) {
                Some(channel) => channel,
                None => return err_msg(header, ErrorKind::InvalidChannel),
            };

            match header.kind() {
                Kind::Request => {
                    if channel.is_at_max_incoming_requests() {
                        return err_msg(header, ErrorKind::RequestLimitExceeded);
                    }

                    if channel.incoming_requests.insert(header.id()) {
                        return err_msg(header, ErrorKind::DuplicateRequest);
                    }
                    channel.increment_cancellation_allowance();

                    // At this point, we have a valid request and its ID has been added to our
                    // incoming set. All we need to do now is to remove it from the buffer.
                    buffer.advance(Header::SIZE);

                    return Success(CompletedRead::NewRequest {
                        id: header.id(),
                        payload: None,
                    });
                }
                Kind::Response => {
                    if !channel.outgoing_requests.remove(&header.id()) {
                        return err_msg(header, ErrorKind::FictitiousRequest);
                    } else {
                        return Success(CompletedRead::ReceivedResponse {
                            id: header.id(),
                            payload: None,
                        });
                    }
                }
                Kind::RequestPl => {
                    // First, we need to "gate" the incoming request; it only gets to bypass the request limit if it is already in progress:
                    let is_new_request = channel.current_multiframe_receive.is_new_transfer(header);

                    if is_new_request {
                        // If we're in the ready state, requests must be eagerly rejected if
                        // exceeding the limit.
                        if channel.is_at_max_incoming_requests() {
                            return err_msg(header, ErrorKind::RequestLimitExceeded);
                        }

                        // We also check for duplicate requests early to avoid reading them.
                        if channel.incoming_requests.contains(&header.id()) {
                            return err_msg(header, ErrorKind::DuplicateRequest);
                        }
                    };

                    let multiframe_outcome: Option<BytesMut> =
                        try_outcome!(channel.current_multiframe_receive.accept(
                            header,
                            &mut buffer,
                            self.max_frame_size,
                            channel.config.max_request_payload_size,
                            ErrorKind::RequestTooLarge
                        ));

                    // If we made it to this point, we have consumed the frame. Record it.
                    if is_new_request {
                        if channel.incoming_requests.insert(header.id()) {
                            return err_msg(header, ErrorKind::DuplicateRequest);
                        }
                        channel.increment_cancellation_allowance();
                    }

                    match multiframe_outcome {
                        Some(payload) => {
                            // Message is complete.
                            return Success(CompletedRead::NewRequest {
                                id: header.id(),
                                payload: Some(payload.freeze()),
                            });
                        }
                        None => {
                            // We need more frames to complete the payload. Do nothing and attempt
                            // to read the next frame.
                        }
                    }
                }
                Kind::ResponsePl => {
                    let is_new_response =
                        channel.current_multiframe_receive.is_new_transfer(header);

                    // Ensure it is not a bogus response.
                    if is_new_response {
                        if !channel.outgoing_requests.contains(&header.id()) {
                            return err_msg(header, ErrorKind::FictitiousRequest);
                        }
                    }

                    let multiframe_outcome: Option<BytesMut> =
                        try_outcome!(channel.current_multiframe_receive.accept(
                            header,
                            &mut buffer,
                            self.max_frame_size,
                            channel.config.max_response_payload_size,
                            ErrorKind::ResponseTooLarge
                        ));

                    // If we made it to this point, we have consumed the frame.
                    if is_new_response {
                        if !channel.outgoing_requests.remove(&header.id()) {
                            return err_msg(header, ErrorKind::FictitiousRequest);
                        }
                    }

                    match multiframe_outcome {
                        Some(payload) => {
                            // Message is complete.
                            return Success(CompletedRead::ReceivedResponse {
                                id: header.id(),
                                payload: Some(payload.freeze()),
                            });
                        }
                        None => {
                            // We need more frames to complete the payload. Do nothing and attempt
                            // to read the next frame.
                        }
                    }
                }
                Kind::CancelReq => {
                    // Cancellations can be sent multiple times and are not checked to avoid
                    // cancellation races. For security reasons they are subject to an allowance.

                    if channel.cancellation_allowance == 0 {
                        return err_msg(header, ErrorKind::CancellationLimitExceeded);
                    }
                    channel.cancellation_allowance -= 1;

                    // TODO: What to do with partially received multi-frame request?

                    return Success(CompletedRead::RequestCancellation { id: header.id() });
                }
                Kind::CancelResp => {
                    if channel.outgoing_requests.remove(&header.id()) {
                        return Success(CompletedRead::ResponseCancellation { id: header.id() });
                    } else {
                        return err_msg(header, ErrorKind::FictitiousCancel);
                    }
                }
            }
        }
    }
}

/// Turn a header and an [`ErrorKind`] into an outgoing message.
///
/// Pure convenience function for the common use case of producing a response message from a
/// received header with an appropriate error.
#[inline(always)]
fn err_msg<T>(header: Header, kind: ErrorKind) -> Outcome<T, OutgoingMessage> {
    Fatal(OutgoingMessage::new(header.with_err(kind), None))
}
