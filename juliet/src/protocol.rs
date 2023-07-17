//! Protocol parsing state machine.
//!
//! The [`JulietProtocol`] type is designed to encapsulate the entire juliet protocol without any
//! dependencies on IO facilities; it can thus be dropped into almost any environment (`std::io`,
//! various `async` runtimes, etc.) with no changes.
//!
//! ## Usage
//!
//! An instance of [`JulietProtocol<N>`] must be created using [`JulietProtocol<N>::builder`], the
//! resulting builder can be used to fine-tune the configuration of the given protocol. The
//! parameter `N` denotes the number of valid channels, which must be set at compile time. See the
//! types documentation for more details.
//!
//! ## Efficiency
//!
//! In general, all bulky data used in the protocol is as zero-copy as possible, for example large
//! messages going out in multiple frames will still share the one original payload buffer passed in
//! at construction. The "exception" to this is the re-assembly of multi-frame messages, which
//! causes fragments to be copied once to form a continguous byte sequence for the payload to avoid
//! memory-exhaustion attacks based on the semtantics of the underlying [`bytes::BytesMut`].

mod multiframe;
mod outgoing_message;

use std::{collections::HashSet, num::NonZeroU32};

use bytes::{Buf, Bytes, BytesMut};
use thiserror::Error;

use self::multiframe::MultiframeReceiver;
pub use self::outgoing_message::{FrameIter, OutgoingFrame, OutgoingMessage};
use crate::{
    header::{self, ErrorKind, Header, Kind},
    try_outcome,
    util::Index,
    varint::{decode_varint32, Varint32},
    ChannelConfiguration, ChannelId, Id,
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
/// `N` denotes the number of valid channels, which should be fixed and agreed upon by both peers
/// prior to initialization.
///
/// ## Input
///
/// This type does not handle IO, rather it expects a growing [`BytesMut`] buffer to be passed in,
/// containing incoming data, using the [`JulietProtocol::process_incoming`] method.
///
/// ## Output
///
/// Multiple methods create [`OutgoingMessage`] values:
///
/// * [`JulietProtocol::create_request`]
/// * [`JulietProtocol::create_response`]
/// * [`JulietProtocol::cancel_request`]
/// * [`JulietProtocol::cancel_response`]
/// * [`JulietProtocol::custom_error`]
///
/// Their return types are usually converted into frames via [`OutgoingMessage::frames()`] and need
/// to be sent to the peer.
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
/// handling multiple connections, as its [`ProtocolBuilder::build()`] method can be reused for
/// every new connection instance.
#[derive(Debug)]
pub struct ProtocolBuilder<const N: usize> {
    /// Configuration for every channel.
    channel_config: [ChannelConfiguration; N],
    /// Maximum frame size.
    max_frame_size: u32,
}

impl<const N: usize> Default for ProtocolBuilder<N> {
    #[inline]
    fn default() -> Self {
        Self::with_default_channel_config(Default::default())
    }
}

impl<const N: usize> ProtocolBuilder<N> {
    /// Creates a new protocol builder with all channels preconfigured using the given config.
    #[inline]
    pub fn with_default_channel_config(config: ChannelConfiguration) -> Self {
        Self {
            channel_config: [config; N],
            max_frame_size: 4096,
        }
    }

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

    /// Sets the maximum frame size.
    ///
    /// # Panics
    ///
    /// Will panic if the maximum size is too small to holder a header, payload length and at least
    /// one byte of payload.
    pub fn max_frame_size(mut self, max_frame_size: u32) -> Self {
        assert!(max_frame_size as usize > Header::SIZE + Varint32::MAX_LEN);

        self.max_frame_size = max_frame_size;
        self
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
#[derive(Debug)]
pub enum CompletedRead {
    /// An error has been received.
    ///
    /// The connection on our end should be closed, the peer will do the same.
    ErrorReceived {
        /// The error header.
        header: Header,
        /// The error data (only with [`ErrorKind::Other`]).
        data: Option<Bytes>,
    },
    /// A new request has been received.
    NewRequest {
        /// The channel of the request.
        channel: ChannelId,
        /// The ID of the request.
        id: Id,
        /// Request payload.
        payload: Option<Bytes>,
    },
    /// A response to one of our requests has been received.
    ReceivedResponse {
        /// The channel of the response.
        channel: ChannelId,
        /// The ID of the request received.
        id: Id,
        /// The response payload.
        payload: Option<Bytes>,
    },
    /// A request was cancelled by the peer.
    RequestCancellation {
        /// The channel of the request cancellation.
        channel: ChannelId,
        /// ID of the request to be cancelled.
        id: Id,
    },
    /// A response was cancelled by the peer.
    ResponseCancellation {
        /// The channel of the response cancellation.
        channel: ChannelId,
        /// The ID of the response to be cancelled.
        id: Id,
    },
}

/// The caller of the this crate has violated the protocol.
///
/// A correct implementation of a client should never encounter this, thus simply unwrapping every
/// instance of this as part of a `Result<_, LocalProtocolViolation>` is usually a valid choice.
///
/// Higher level layers like [`rpc`](crate::rpc) should make it impossible to encounter
/// [`LocalProtocolViolation`]s.
#[derive(Copy, Clone, Debug, Error)]
pub enum LocalProtocolViolation {
    /// A request was not sent because doing so would exceed the request limit on channel.
    ///
    /// Wait for addtional requests to be cancelled or answered. Calling
    /// [`JulietProtocol::allowed_to_send_request()`] beforehand is recommended.
    #[error("sending would exceed request limit")]
    WouldExceedRequestLimit,
    /// The channel given does not exist.
    ///
    /// The given [`ChannelId`] exceeds `N` of [`JulietProtocol<N>`].
    #[error("invalid channel")]
    InvalidChannel(ChannelId),
    /// The given payload exceeds the configured limit.
    ///
    /// See [`ChannelConfiguration::with_max_request_payload_size()`] and
    /// [`ChannelConfiguration::with_max_response_payload_size()`] for details.
    #[error("payload exceeds configured limit")]
    PayloadExceedsLimit,
    /// The given error payload exceeds a single frame.
    ///
    /// Error payloads may not span multiple frames, shorten the payload or increase frame size.
    #[error("error payload would be multi-frame")]
    ErrorPayloadIsMultiFrame,
}

macro_rules! log_frame {
    ($header:expr) => {
        #[cfg(feature = "tracing")]
        {
            use tracing::trace;
            trace!(header=%$header, "received");
        }
        #[cfg(not(feature = "tracing"))]
        {
            // tracing feature disabled, not logging frame
        }
    };
    ($header:expr, $payload:expr) => {
        #[cfg(feature = "tracing")]
        {
            use tracing::trace;
            trace!(header=%$header, payload=%crate::util::PayloadFormat(&$payload), "received");
        }
        #[cfg(not(feature = "tracing"))]
        {
            // tracing feature disabled, not logging frame
        }
    };
}

impl<const N: usize> JulietProtocol<N> {
    /// Creates a new juliet protocol builder instance.
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

    /// Returns the configured maximum frame size.
    #[inline(always)]
    pub fn max_frame_size(&self) -> u32 {
        self.max_frame_size
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
    /// If the ID is not in the outgoing set, due to already being responded to or cancelled, `None`
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
    pub fn custom_error(
        &mut self,
        channel: ChannelId,
        id: Id,
        payload: Bytes,
    ) -> Result<OutgoingMessage, LocalProtocolViolation> {
        let header = Header::new_error(header::ErrorKind::Other, channel, id);

        let msg = OutgoingMessage::new(header, Some(payload));
        if msg.is_multi_frame(self.max_frame_size) {
            Err(LocalProtocolViolation::ErrorPayloadIsMultiFrame)
        } else {
            Ok(msg)
        }
    }

    /// Processes incoming data from a buffer.
    ///
    /// This is the main ingress function of [`JulietProtocol`]. `buffer` should continuously be
    /// appended with all incoming data; the [`Outcome`] returned indicates when the function should
    /// be called next:
    ///
    /// * [`Outcome::Success`] indicates `process_incoming` should be called again as early as
    ///   possible, since additional messages may already be contained in `buffer`.
    /// * [`Outcome::Incomplete`] tells the caller to not call `process_incoming` again before at
    ///   least `n` additional bytes have been added to buffer.
    /// * [`Outcome::Fatal`] indicates that the remote peer violated the protocol, the returned
    ///   [`Header`] should be attempted to be sent to the peer before the connection is being
    ///   closed.
    ///
    /// This method transparently handles multi-frame sends, any incomplete messages will be
    /// buffered internally until they are complete.
    ///
    /// Any successful frame read will cause `buffer` to be advanced by the length of the frame,
    /// thus eventually freeing the data if not held elsewhere.
    ///
    /// **Important**: This functions `Err` value is an [`OutgoingMessage`] to be sent to the peer.
    /// It must be the final message sent and should be sent as soon as possible, with the
    /// connection being close afterwards.
    pub fn process_incoming(
        &mut self,
        buffer: &mut BytesMut,
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
                    #[cfg(feature = "tracing")]
                    tracing::trace!(?header_raw, "received invalid header");
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
                        // The error data is varint encoded, but must not exceed a single frame.
                        let tail = &buffer[Header::SIZE..];

                        // This can be confusing for the other end, receiving an error for their
                        // error, but they should not send malformed errors in the first place!
                        let parsed_length =
                            try_outcome!(decode_varint32(tail).map_err(|_overflow| {
                                OutgoingMessage::new(header.with_err(ErrorKind::BadVarInt), None)
                            }));

                        // Create indices into buffer.
                        let preamble_end =
                            Index::new(buffer, Header::SIZE + parsed_length.offset.get() as usize);
                        let payload_length = parsed_length.value as usize;
                        let frame_end = Index::new(buffer, *preamble_end + payload_length);

                        // No multi-frame messages allowed!
                        if *frame_end > self.max_frame_size as usize {
                            return err_msg(header, ErrorKind::SegmentViolation);
                        }

                        if buffer.len() < *frame_end {
                            return Outcome::incomplete(*frame_end - buffer.len());
                        }

                        buffer.advance(*preamble_end);
                        let payload = buffer.split_to(payload_length).freeze();

                        log_frame!(header, payload);
                        return Success(CompletedRead::ErrorReceived {
                            header,
                            data: Some(payload),
                        });
                    }
                    _ => {
                        log_frame!(header);
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

                    if !channel.incoming_requests.insert(header.id()) {
                        return err_msg(header, ErrorKind::DuplicateRequest);
                    }
                    channel.increment_cancellation_allowance();

                    // At this point, we have a valid request and its ID has been added to our
                    // incoming set. All we need to do now is to remove it from the buffer.
                    buffer.advance(Header::SIZE);

                    log_frame!(header);
                    return Success(CompletedRead::NewRequest {
                        channel: header.channel(),
                        id: header.id(),
                        payload: None,
                    });
                }
                Kind::Response => {
                    if !channel.outgoing_requests.remove(&header.id()) {
                        return err_msg(header, ErrorKind::FictitiousRequest);
                    } else {
                        log_frame!(header);
                        return Success(CompletedRead::ReceivedResponse {
                            channel: header.channel(),
                            id: header.id(),
                            payload: None,
                        });
                    }
                }
                Kind::RequestPl => {
                    // Make a note whether or not we are continueing an existing request.
                    let is_new_request = channel.current_multiframe_receive.is_new_transfer(header);

                    let multiframe_outcome: Option<BytesMut> =
                        try_outcome!(channel.current_multiframe_receive.accept(
                            header,
                            buffer,
                            self.max_frame_size,
                            channel.config.max_request_payload_size,
                            ErrorKind::RequestTooLarge
                        ));

                    // If we made it to this point, we have consumed the frame. Record it.

                    if is_new_request {
                        // Requests must be eagerly (first frame) rejected if exceeding the limit.
                        if channel.is_at_max_incoming_requests() {
                            return err_msg(header, ErrorKind::RequestLimitExceeded);
                        }

                        // We also check for duplicate requests early to avoid reading them.
                        if !channel.incoming_requests.insert(header.id()) {
                            return err_msg(header, ErrorKind::DuplicateRequest);
                        }
                        channel.increment_cancellation_allowance();
                    }

                    if let Some(payload) = multiframe_outcome {
                        // Message is complete.
                        let payload = payload.freeze();

                        return Success(CompletedRead::NewRequest {
                            channel: header.channel(),
                            id: header.id(),
                            payload: Some(payload),
                        });
                    } else {
                        // We need more frames to complete the payload. Do nothing and attempt
                        // to read the next frame.
                    }
                }
                Kind::ResponsePl => {
                    let is_new_response =
                        channel.current_multiframe_receive.is_new_transfer(header);

                    // Ensure it is not a bogus response.
                    if is_new_response && !channel.outgoing_requests.contains(&header.id()) {
                        return err_msg(header, ErrorKind::FictitiousRequest);
                    }

                    let multiframe_outcome: Option<BytesMut> =
                        try_outcome!(channel.current_multiframe_receive.accept(
                            header,
                            buffer,
                            self.max_frame_size,
                            channel.config.max_response_payload_size,
                            ErrorKind::ResponseTooLarge
                        ));

                    // If we made it to this point, we have consumed the frame.
                    if is_new_response && !channel.outgoing_requests.remove(&header.id()) {
                        return err_msg(header, ErrorKind::FictitiousRequest);
                    }

                    if let Some(payload) = multiframe_outcome {
                        // Message is complete.
                        let payload = payload.freeze();

                        return Success(CompletedRead::ReceivedResponse {
                            channel: header.channel(),
                            id: header.id(),
                            payload: Some(payload),
                        });
                    } else {
                        // We need more frames to complete the payload. Do nothing and attempt
                        // to read the next frame.
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
                    // TODO: Actually remove from incoming set.

                    #[cfg(feature = "tracing")]
                    {
                        use tracing::trace;
                        trace!(%header, "received request cancellation");
                    }

                    return Success(CompletedRead::RequestCancellation {
                        channel: header.channel(),
                        id: header.id(),
                    });
                }
                Kind::CancelResp => {
                    if channel.outgoing_requests.remove(&header.id()) {
                        log_frame!(header);
                        return Success(CompletedRead::ResponseCancellation {
                            channel: header.channel(),
                            id: header.id(),
                        });
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
    log_frame!(header);
    Fatal(OutgoingMessage::new(header.with_err(kind), None))
}

/// Determines whether or not a payload with the given size is a multi-frame payload when sent
/// using the provided maximum frame size.
///
/// # Panics
///
/// Panics in debug mode if the given payload length is larger than `u32::MAX`.
#[inline]
pub fn payload_is_multi_frame(max_frame_size: u32, payload_len: usize) -> bool {
    debug_assert!(
        payload_len <= u32::MAX as usize,
        "payload cannot exceed `u32::MAX`"
    );

    payload_len as u64 + Header::SIZE as u64 + (Varint32::encode(payload_len as u32)).len() as u64
        > max_frame_size as u64
}
