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
//! type's documentation for more details.
//!
//! ## Efficiency
//!
//! In general, all bulky data used in the protocol is as zero-copy as possible, for example large
//! messages going out in multiple frames will still share the one original payload buffer passed in
//! at construction. The "exception" to this is the re-assembly of multi-frame messages, which
//! causes fragments to be copied once to form a contiguous byte sequence for the payload to avoid
//! memory-exhaustion attacks based on the semantics of the underlying [`bytes::BytesMut`].

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

/// A channel ID to fill in when the channel is actually unknown or not relevant.
///
/// Note that this is not a reserved channel, just a default chosen -- it may clash with an
/// actually active channel.
const UNKNOWN_CHANNEL: ChannelId = ChannelId::new(0);

/// An ID to fill in when the ID should not matter.
///
/// Not a reserved id, it may clash with existing ones.
const UNKNOWN_ID: Id = Id::new(0);

/// Maximum frame size.
///
/// The maximum configured frame size is subject to some invariants and is wrapped into a newtype
/// for convenience.
#[derive(Copy, Clone, Debug)]
#[repr(transparent)]
pub struct MaxFrameSize(u32);

impl MaxFrameSize {
    /// The minimum sensible frame size maximum.
    ///
    /// Set to fit at least a full preamble and a single byte of payload.
    pub const MIN: u32 = Header::SIZE as u32 + Varint32::MAX_LEN as u32 + 1;

    /// Recommended default for the maximum frame size.
    ///
    /// Chosen according to the Juliet RFC.
    pub const DEFAULT: MaxFrameSize = MaxFrameSize(4096);

    /// Constructs a new maximum frame size.
    ///
    /// # Panics
    ///
    /// Will panic if the given maximum frame size is less than [`MaxFrameSize::MIN`].
    #[inline(always)]
    pub const fn new(max_frame_size: u32) -> Self {
        assert!(
            max_frame_size >= Self::MIN,
            "given maximum frame size is below permissible minimum for maximum frame size"
        );
        MaxFrameSize(max_frame_size)
    }

    /// Returns the maximum frame size.
    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Returns the maximum frame size cast as `usize`.
    #[inline(always)]
    pub const fn get_usize(self) -> usize {
        // Safe cast on all 32-bit and up systems.
        self.0 as usize
    }

    /// Returns the maximum frame size without the header size.
    #[inline(always)]
    pub const fn without_header(self) -> usize {
        self.get_usize() - Header::SIZE
    }
}

impl Default for MaxFrameSize {
    #[inline(always)]
    fn default() -> Self {
        MaxFrameSize::DEFAULT
    }
}

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
#[cfg_attr(test, derive(Clone))]
pub struct JulietProtocol<const N: usize> {
    /// Bi-directional channels.
    channels: [Channel; N],
    /// The maximum size for a single frame.
    max_frame_size: MaxFrameSize,
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
    max_frame_size: MaxFrameSize,
}

impl<const N: usize> Default for ProtocolBuilder<N> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ProtocolBuilder<N> {
    /// Creates a new protocol builder with default configuration for every channel.
    pub const fn new() -> Self {
        Self::with_default_channel_config(ChannelConfiguration::new())
    }

    /// Creates a new protocol builder with all channels preconfigured using the given config.
    #[inline]
    pub const fn with_default_channel_config(config: ChannelConfiguration) -> Self {
        Self {
            channel_config: [config; N],
            max_frame_size: MaxFrameSize::DEFAULT,
        }
    }

    /// Update the channel configuration for a given channel.
    pub const fn channel_config(
        mut self,
        channel: ChannelId,
        config: ChannelConfiguration,
    ) -> Self {
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
    /// Will panic if the maximum size is too small to hold a header, payload length and at least
    /// one byte of payload (see [`MaxFrameSize::MIN`]).
    pub const fn max_frame_size(mut self, max_frame_size: u32) -> Self {
        self.max_frame_size = MaxFrameSize::new(max_frame_size);
        self
    }
}

/// Per-channel data.
///
/// Used internally by the protocol to keep track. This data structure closely tracks the
/// information specified in the juliet RFC.
#[derive(Debug)]
#[cfg_attr(test, derive(Clone))]
struct Channel {
    /// A set of request IDs from requests received that have not been answered with a response or
    /// cancellation yet.
    incoming_requests: HashSet<Id>,
    /// A set of request IDs of requests made, for which no response or cancellation has been
    /// received yet.
    outgoing_requests: HashSet<Id>,
    /// The multiframe receiver state machine.
    ///
    /// Every channel allows for at most one multi-frame message to be in progress at the same
    /// time.
    current_multiframe_receiver: MultiframeReceiver,
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
            current_multiframe_receiver: MultiframeReceiver::default(),
            cancellation_allowance: 0,
            config,
            prev_request_id: 0,
        }
    }

    /// Returns whether or not the peer has exhausted the number of in-flight requests allowed.
    #[inline]
    pub fn is_at_max_incoming_requests(&self) -> bool {
        self.incoming_requests.len() >= self.config.request_limit as usize
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

    /// Creates a new request, bypassing all client-side checks.
    ///
    /// Low-level function that does nothing but create a syntactically correct request and track
    /// its outgoing ID. This function is not meant to be called outside of this module or its unit
    /// tests. See [`JulietProtocol::create_request`] instead.
    #[inline(always)]
    fn create_unchecked_request(
        &mut self,
        channel_id: ChannelId,
        payload: Option<Bytes>,
    ) -> OutgoingMessage {
        // The `unwrap_or` below should never be triggered, as long as `u16::MAX` or less
        // requests are currently in flight, which is always the case with safe API use.
        let id = self.generate_request_id().unwrap_or(Id(0));

        // Record the outgoing request for later.
        self.outgoing_requests.insert(id);

        if let Some(payload) = payload {
            let header = Header::new(header::Kind::RequestPl, channel_id, id);
            OutgoingMessage::new(header, Some(payload))
        } else {
            let header = Header::new(header::Kind::Request, channel_id, id);
            OutgoingMessage::new(header, None)
        }
    }
}

/// Creates a new response without checking or altering channel states.
///
/// Low-level function exposed for testing. Does not affect the tracking of IDs, thus can be used to
/// send duplicate or ficticious responses.
#[inline(always)]
fn create_unchecked_response(
    channel: ChannelId,
    id: Id,
    payload: Option<Bytes>,
) -> OutgoingMessage {
    if let Some(payload) = payload {
        let header = Header::new(header::Kind::ResponsePl, channel, id);
        OutgoingMessage::new(header, Some(payload))
    } else {
        let header = Header::new(header::Kind::Response, channel, id);
        OutgoingMessage::new(header, None)
    }
}

/// Creates a request cancellation without checks.
///
/// Low-level function exposed for testing. Does not verify that the given request exists or has not
/// been cancelled before.
#[inline(always)]
fn create_unchecked_request_cancellation(channel: ChannelId, id: Id) -> OutgoingMessage {
    let header = Header::new(header::Kind::CancelReq, channel, id);
    OutgoingMessage::new(header, None)
}

/// Creates a response cancellation without checks.
///
/// Low-level function exposed for testing. Does not verify that the given request has been received
/// or a response sent already.
fn create_unchecked_response_cancellation(channel: ChannelId, id: Id) -> OutgoingMessage {
    let header = Header::new(header::Kind::CancelResp, channel, id);
    OutgoingMessage::new(header, None)
}

/// A successful read from the peer.
#[must_use]
#[derive(Debug, Eq, PartialEq)]
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
#[derive(Copy, Clone, Debug, Eq, Error, PartialEq)]
pub enum LocalProtocolViolation {
    /// A request was not sent because doing so would exceed the request limit on channel.
    ///
    /// Wait for additional requests to be cancelled or answered. Calling
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
    };
    ($header:expr, $payload:expr) => {
        #[cfg(feature = "tracing")]
        {
            use tracing::trace;
            trace!(header=%$header, payload=%crate::util::PayloadFormat(&$payload), "received");
        }
    };
}

impl<const N: usize> JulietProtocol<N> {
    /// Creates a new juliet protocol builder instance.
    #[inline]
    pub const fn builder(config: ChannelConfiguration) -> ProtocolBuilder<N> {
        ProtocolBuilder {
            channel_config: [config; N],
            max_frame_size: MaxFrameSize::DEFAULT,
        }
    }

    /// Looks up a given channel by ID.
    ///
    /// Returns a `LocalProtocolViolation` if called with non-existent channel.
    #[inline(always)]
    const fn lookup_channel(&self, channel: ChannelId) -> Result<&Channel, LocalProtocolViolation> {
        if channel.0 as usize >= N {
            Err(LocalProtocolViolation::InvalidChannel(channel))
        } else {
            Ok(&self.channels[channel.0 as usize])
        }
    }

    /// Looks up a given channel by ID, mutably.
    ///
    /// Returns a `LocalProtocolViolation` if called with non-existent channel.
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
    pub const fn max_frame_size(&self) -> MaxFrameSize {
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

        Ok(chan.create_unchecked_request(channel, payload))
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

        Ok(Some(create_unchecked_response(channel, id, payload)))
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

        if !chan.outgoing_requests.contains(&id) {
            // The request has received a response already, no need to cancel. Note that merely
            // sending the cancellation is not enough here, we still expect either cancellation or
            // response from the peer.
            return Ok(None);
        }

        Ok(Some(create_unchecked_request_cancellation(channel, id)))
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

        Ok(Some(create_unchecked_response_cancellation(channel, id)))
    }

    /// Creates an error message with type [`ErrorKind::Other`].
    ///
    /// The resulting [`OutgoingMessage`] is the last message that should be sent to the peer, the
    /// caller should ensure no more messages are sent.
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
                        if *frame_end > self.max_frame_size.get_usize() {
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

                        buffer.advance(Header::SIZE);
                        return Success(CompletedRead::ReceivedResponse {
                            channel: header.channel(),
                            id: header.id(),
                            payload: None,
                        });
                    }
                }
                Kind::RequestPl => {
                    // Make a note whether or not we are continuing an existing request.
                    let is_new_request =
                        channel.current_multiframe_receiver.is_new_transfer(header);

                    let multiframe_outcome: Option<BytesMut> =
                        try_outcome!(channel.current_multiframe_receiver.accept(
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
                        channel.current_multiframe_receiver.is_new_transfer(header);

                    // Ensure it is not a bogus response.
                    if is_new_response && !channel.outgoing_requests.contains(&header.id()) {
                        return err_msg(header, ErrorKind::FictitiousRequest);
                    }

                    let multiframe_outcome: Option<BytesMut> =
                        try_outcome!(channel.current_multiframe_receiver.accept(
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
                    buffer.advance(Header::SIZE);

                    #[cfg(feature = "tracing")]
                    {
                        use tracing::trace;
                        trace!(%header, "received request cancellation");
                    }

                    // Multi-frame transfers that have not yet been completed are a special case,
                    // since they have never been reported, we can cancel these internally.
                    if let Some(in_progress_header) =
                        channel.current_multiframe_receiver.in_progress_header()
                    {
                        // We already know it is a cancellation and we are on the correct channel.
                        if in_progress_header.id() == header.id() {
                            // Cancel transfer.
                            channel.current_multiframe_receiver = MultiframeReceiver::default();
                            // Remove tracked request.
                            channel.incoming_requests.remove(&header.id());
                        }
                    }

                    // Check incoming request. If it was already cancelled or answered, ignore, as
                    // it is valid to send wrong cancellation up to the cancellation allowance.
                    //
                    // An incoming request may have also already been answered, which is also
                    // reason to ignore it.
                    //
                    // However, we cannot remove it here, as we need to track whether we have sent
                    // something back.
                    if !channel.incoming_requests.contains(&header.id()) {
                        // Already answered, ignore the late cancellation.
                    } else {
                        return Success(CompletedRead::RequestCancellation {
                            channel: header.channel(),
                            id: header.id(),
                        });
                    }
                }
                Kind::CancelResp => {
                    if channel.outgoing_requests.remove(&header.id()) {
                        log_frame!(header);
                        buffer.advance(Header::SIZE);

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
pub const fn payload_is_multi_frame(max_frame_size: MaxFrameSize, payload_len: usize) -> bool {
    debug_assert!(
        payload_len <= u32::MAX as usize,
        "payload cannot exceed `u32::MAX`"
    );

    payload_len as u64 + Header::SIZE as u64 + (Varint32::encode(payload_len as u32)).len() as u64
        > max_frame_size.get() as u64
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fmt::Debug, ops::Not};

    use assert_matches::assert_matches;
    use bytes::{Buf, Bytes, BytesMut};
    use proptest_attr_macro::proptest;
    use proptest_derive::Arbitrary;
    use static_assertions::const_assert;
    use strum::{EnumIter, IntoEnumIterator};

    use crate::{
        header::{ErrorKind, Header, Kind},
        protocol::{
            create_unchecked_response, multiframe::MultiframeReceiver, payload_is_multi_frame,
            CompletedRead, LocalProtocolViolation,
        },
        varint::Varint32,
        ChannelConfiguration, ChannelId, Id, Outcome,
    };

    use super::{
        create_unchecked_request_cancellation, create_unchecked_response_cancellation, err_msg,
        Channel, JulietProtocol, MaxFrameSize, OutgoingMessage, ProtocolBuilder,
    };

    /// A generic payload that can be used in testing.
    #[derive(Arbitrary, Clone, Copy, Debug, EnumIter)]
    enum VaryingPayload {
        /// No payload at all.
        None,
        /// A payload that fits into a single frame (using `TestingSetup`'s defined limits).
        SingleFrame,
        /// A payload that spans more than one frame.
        MultiFrame,
        /// A payload that exceeds the request size limit.
        TooLarge,
    }

    impl VaryingPayload {
        /// Returns all valid payload sizes.
        fn all_valid() -> impl Iterator<Item = Self> {
            [
                VaryingPayload::None,
                VaryingPayload::SingleFrame,
                VaryingPayload::MultiFrame,
            ]
            .into_iter()
        }

        /// Returns whether the resulting payload would be `Option::None`.
        fn is_none(self) -> bool {
            match self {
                VaryingPayload::None => true,
                VaryingPayload::SingleFrame => false,
                VaryingPayload::MultiFrame => false,
                VaryingPayload::TooLarge => false,
            }
        }

        /// Returns the kind header required if this payload is used in a request.
        fn request_kind(self) -> Kind {
            if self.is_none() {
                Kind::Request
            } else {
                Kind::RequestPl
            }
        }

        /// Returns the kind header required if this payload is used in a response.
        fn response_kind(self) -> Kind {
            if self.is_none() {
                Kind::Response
            } else {
                Kind::ResponsePl
            }
        }

        /// Produce the actual payload.
        fn get(self) -> Option<Bytes> {
            self.get_slice().map(Bytes::from_static)
        }

        /// Produce the payloads underlying slice.
        fn get_slice(self) -> Option<&'static [u8]> {
            const SHORT_PAYLOAD: &[u8] = b"asdf";
            const_assert!(
                SHORT_PAYLOAD.len()
                    <= TestingSetup::MAX_FRAME_SIZE as usize - Header::SIZE - Varint32::MAX_LEN
            );

            const LONG_PAYLOAD: &[u8] =
            b"large payload large payload large payload large payload large payload large payload";
            const_assert!(LONG_PAYLOAD.len() > TestingSetup::MAX_FRAME_SIZE as usize);

            const OVERLY_LONG_PAYLOAD: &[u8] = b"abcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefghabcdefgh";
            const_assert!(OVERLY_LONG_PAYLOAD.len() > TestingSetup::MAX_PAYLOAD_SIZE as usize);

            match self {
                VaryingPayload::None => None,
                VaryingPayload::SingleFrame => Some(SHORT_PAYLOAD),
                VaryingPayload::MultiFrame => Some(LONG_PAYLOAD),
                VaryingPayload::TooLarge => Some(OVERLY_LONG_PAYLOAD),
            }
        }
    }

    #[test]
    fn max_frame_size_works() {
        let sz = MaxFrameSize::new(1234);
        assert_eq!(sz.get(), 1234);
        assert_eq!(sz.without_header(), 1230);

        // Smallest allowed:
        assert_eq!(MaxFrameSize::MIN, 10);
        let small = MaxFrameSize::new(10);
        assert_eq!(small.get(), 10);
        assert_eq!(small.without_header(), 6);
    }

    #[test]
    #[should_panic(expected = "permissible minimum for maximum frame size")]
    fn max_frame_size_panics_on_too_small_size() {
        MaxFrameSize::new(MaxFrameSize::MIN - 1);
    }

    #[test]
    fn request_id_generation_generates_unique_ids() {
        let mut channel = Channel::new(Default::default());

        // IDs are sequential.
        assert_eq!(channel.generate_request_id(), Some(Id::new(1)));
        assert_eq!(channel.generate_request_id(), Some(Id::new(2)));
        assert_eq!(channel.generate_request_id(), Some(Id::new(3)));

        // Manipulate internal counter, expecting rollover.
        channel.prev_request_id = u16::MAX - 2;
        assert_eq!(channel.generate_request_id(), Some(Id::new(u16::MAX - 1)));
        assert_eq!(channel.generate_request_id(), Some(Id::new(u16::MAX)));
        assert_eq!(channel.generate_request_id(), Some(Id::new(0)));
        assert_eq!(channel.generate_request_id(), Some(Id::new(1)));

        // Insert some request IDs to mark them as used, causing them to be skipped.
        channel.outgoing_requests.extend([1, 2, 3, 5].map(Id::new));
        assert_eq!(channel.generate_request_id(), Some(Id::new(4)));
        assert_eq!(channel.generate_request_id(), Some(Id::new(6)));
    }

    #[test]
    fn allowed_to_send_throttles_when_appropriate() {
        // A channel with a request limit of 0 is unusable, but legal.
        assert!(
            !Channel::new(ChannelConfiguration::new().with_request_limit(0))
                .allowed_to_send_request()
        );

        // Capacity: 1
        let mut channel = Channel::new(ChannelConfiguration::new().with_request_limit(1));
        assert!(channel.allowed_to_send_request());

        // Incoming requests should not affect this.
        channel.incoming_requests.insert(Id::new(1234));
        channel.incoming_requests.insert(Id::new(5678));
        channel.incoming_requests.insert(Id::new(9010));
        assert!(channel.allowed_to_send_request());

        // Fill up capacity.
        channel.outgoing_requests.insert(Id::new(1));
        assert!(!channel.allowed_to_send_request());

        // Capacity: 2
        let mut channel = Channel::new(ChannelConfiguration::new().with_request_limit(2));
        assert!(channel.allowed_to_send_request());
        channel.outgoing_requests.insert(Id::new(1));
        assert!(channel.allowed_to_send_request());
        channel.outgoing_requests.insert(Id::new(2));
        assert!(!channel.allowed_to_send_request());
    }

    #[test]
    fn is_at_max_incoming_requests_works() {
        // A channel with a request limit of 0 is legal.
        assert!(
            Channel::new(ChannelConfiguration::new().with_request_limit(0))
                .is_at_max_incoming_requests()
        );

        // Capacity: 1
        let mut channel = Channel::new(ChannelConfiguration::new().with_request_limit(1));
        assert!(!channel.is_at_max_incoming_requests());

        // Inserting outgoing requests should not prompt any change to incoming.
        channel.outgoing_requests.insert(Id::new(1234));
        channel.outgoing_requests.insert(Id::new(4567));
        assert!(!channel.is_at_max_incoming_requests());

        channel.incoming_requests.insert(Id::new(1));
        assert!(channel.is_at_max_incoming_requests());

        // Capacity: 2
        let mut channel = Channel::new(ChannelConfiguration::new().with_request_limit(2));
        assert!(!channel.is_at_max_incoming_requests());
        channel.incoming_requests.insert(Id::new(1));
        assert!(!channel.is_at_max_incoming_requests());
        channel.incoming_requests.insert(Id::new(2));
        assert!(channel.is_at_max_incoming_requests());
    }

    #[test]
    fn cancellation_allowance_incrementation_works() {
        // With a 0 request limit, we also don't allow any cancellations.
        let mut channel = Channel::new(ChannelConfiguration::new().with_request_limit(0));
        channel.increment_cancellation_allowance();

        assert_eq!(channel.cancellation_allowance, 0);

        // Ensure that the cancellation allowance cannot exceed request limit.
        let mut channel = Channel::new(ChannelConfiguration::new().with_request_limit(3));
        channel.increment_cancellation_allowance();
        assert_eq!(channel.cancellation_allowance, 1);
        channel.increment_cancellation_allowance();
        assert_eq!(channel.cancellation_allowance, 2);
        channel.increment_cancellation_allowance();
        assert_eq!(channel.cancellation_allowance, 3);
        channel.increment_cancellation_allowance();
        assert_eq!(channel.cancellation_allowance, 3);
        channel.increment_cancellation_allowance();
        assert_eq!(channel.cancellation_allowance, 3);
    }

    #[test]
    fn test_channel_lookups_work() {
        let mut protocol: JulietProtocol<3> = ProtocolBuilder::new().build();

        // We mark channels by inserting an ID into them, that way we can ensure we're not getting
        // back the same channel every time.
        protocol
            .lookup_channel_mut(ChannelId(0))
            .expect("channel missing")
            .outgoing_requests
            .insert(Id::new(100));
        protocol
            .lookup_channel_mut(ChannelId(1))
            .expect("channel missing")
            .outgoing_requests
            .insert(Id::new(101));
        protocol
            .lookup_channel_mut(ChannelId(2))
            .expect("channel missing")
            .outgoing_requests
            .insert(Id::new(102));
        assert!(matches!(
            protocol.lookup_channel_mut(ChannelId(3)),
            Err(LocalProtocolViolation::InvalidChannel(ChannelId(3)))
        ));
        assert!(matches!(
            protocol.lookup_channel_mut(ChannelId(4)),
            Err(LocalProtocolViolation::InvalidChannel(ChannelId(4)))
        ));
        assert!(matches!(
            protocol.lookup_channel_mut(ChannelId(255)),
            Err(LocalProtocolViolation::InvalidChannel(ChannelId(255)))
        ));

        // Now look up the channels and ensure they contain the right values
        assert_eq!(
            protocol
                .lookup_channel(ChannelId(0))
                .expect("channel missing")
                .outgoing_requests,
            HashSet::from([Id::new(100)])
        );
        assert_eq!(
            protocol
                .lookup_channel(ChannelId(1))
                .expect("channel missing")
                .outgoing_requests,
            HashSet::from([Id::new(101)])
        );
        assert_eq!(
            protocol
                .lookup_channel(ChannelId(2))
                .expect("channel missing")
                .outgoing_requests,
            HashSet::from([Id::new(102)])
        );
        assert!(matches!(
            protocol.lookup_channel(ChannelId(3)),
            Err(LocalProtocolViolation::InvalidChannel(ChannelId(3)))
        ));
        assert!(matches!(
            protocol.lookup_channel(ChannelId(4)),
            Err(LocalProtocolViolation::InvalidChannel(ChannelId(4)))
        ));
        assert!(matches!(
            protocol.lookup_channel(ChannelId(255)),
            Err(LocalProtocolViolation::InvalidChannel(ChannelId(255)))
        ));
    }

    #[proptest]
    fn err_msg_works(header: Header) {
        for err_kind in ErrorKind::iter() {
            let outcome = err_msg::<()>(header, err_kind);
            if let Outcome::Fatal(msg) = outcome {
                assert_eq!(msg.header().id(), header.id());
                assert_eq!(msg.header().channel(), header.channel());
                assert!(msg.header().is_error());
                assert_eq!(msg.header().error_kind(), err_kind);
            } else {
                panic!("expected outcome to be fatal");
            }
        }
    }

    #[test]
    fn multi_frame_estimation_works() {
        let max_frame_size = MaxFrameSize::new(512);

        // Note: 512 takes two bytes to encode, so the total overhead is 6 bytes.

        assert!(!payload_is_multi_frame(max_frame_size, 0));
        assert!(!payload_is_multi_frame(max_frame_size, 1));
        assert!(!payload_is_multi_frame(max_frame_size, 5));
        assert!(!payload_is_multi_frame(max_frame_size, 6));
        assert!(!payload_is_multi_frame(max_frame_size, 7));
        assert!(!payload_is_multi_frame(max_frame_size, 505));
        assert!(!payload_is_multi_frame(max_frame_size, 506));
        assert!(payload_is_multi_frame(max_frame_size, 507));
        assert!(payload_is_multi_frame(max_frame_size, 508));
        assert!(payload_is_multi_frame(max_frame_size, u32::MAX as usize));
    }

    #[test]
    fn create_requests_with_correct_input_sets_state_accordingly() {
        for payload in VaryingPayload::all_valid() {
            // Configure a protocol with payload, at least 10 bytes segment size.
            let mut protocol = ProtocolBuilder::<5>::with_default_channel_config(
                ChannelConfiguration::new()
                    .with_request_limit(1)
                    .with_max_request_payload_size(1024),
            )
            .max_frame_size(20)
            .build();

            let channel = ChannelId::new(2);
            let other_channel = ChannelId::new(0);

            assert!(protocol
                .allowed_to_send_request(channel)
                .expect("channel should exist"));

            let req = protocol
                .create_request(channel, payload.get())
                .expect("should be able to create request");

            assert_eq!(req.header().channel(), channel);
            assert_eq!(req.header().kind(), payload.request_kind());

            // We expect exactly one id in the outgoing set.
            assert_eq!(
                protocol
                    .lookup_channel(channel)
                    .expect("should have channel")
                    .outgoing_requests,
                [Id::new(1)].into()
            );

            // We've used up the default limit of one.
            assert!(!protocol
                .allowed_to_send_request(channel)
                .expect("channel should exist"));

            // We should still be able to create requests on a different channel.
            assert!(protocol
                .lookup_channel(other_channel)
                .expect("channel 0 should exist")
                .outgoing_requests
                .is_empty());

            let other_req = protocol
                .create_request(other_channel, payload.get())
                .expect("should be able to create request");

            assert_eq!(other_req.header().channel(), other_channel);
            assert_eq!(other_req.header().kind(), payload.request_kind());

            // We expect exactly one id in the outgoing set of each channel now.
            assert_eq!(
                protocol
                    .lookup_channel(channel)
                    .expect("should have channel")
                    .outgoing_requests,
                [Id::new(1)].into()
            );
            assert_eq!(
                protocol
                    .lookup_channel(other_channel)
                    .expect("should have channel")
                    .outgoing_requests,
                [Id::new(1)].into()
            );
        }
    }

    #[test]
    fn create_requests_with_invalid_inputs_fails() {
        for payload in VaryingPayload::all_valid() {
            // Configure a protocol with payload, at least 10 bytes segment size.
            let mut protocol = ProtocolBuilder::<2>::with_default_channel_config(
                ChannelConfiguration::new()
                    .with_max_request_payload_size(512)
                    .with_max_response_payload_size(512),
            )
            .build();

            let channel = ChannelId::new(1);

            // Try an invalid channel, should result in an error.
            assert!(matches!(
                protocol.create_request(ChannelId::new(2), payload.get()),
                Err(LocalProtocolViolation::InvalidChannel(ChannelId(2)))
            ));

            assert!(protocol
                .allowed_to_send_request(channel)
                .expect("channel should exist"));
            let _ = protocol
                .create_request(channel, payload.get())
                .expect("should be able to create request");

            assert!(matches!(
                protocol.create_request(channel, payload.get()),
                Err(LocalProtocolViolation::WouldExceedRequestLimit)
            ));
        }
    }

    #[test]
    fn create_response_with_correct_input_clears_state_accordingly() {
        for payload in VaryingPayload::all_valid() {
            let mut protocol = ProtocolBuilder::<4>::with_default_channel_config(
                ChannelConfiguration::new()
                    .with_max_request_payload_size(512)
                    .with_max_response_payload_size(512),
            )
            .build();

            let channel = ChannelId::new(3);

            // Inject a channel to have already received two requests.
            let req_id = Id::new(9);
            let leftover_id = Id::new(77);
            protocol
                .lookup_channel_mut(channel)
                .expect("should find channel")
                .incoming_requests
                .extend([req_id, leftover_id]);

            // Responding to a non-existent request should not result in a message.
            assert!(protocol
                .create_response(channel, Id::new(12), payload.get())
                .expect("should allow attempting to respond to non-existent request")
                .is_none());

            // Actual response.
            let resp = protocol
                .create_response(channel, req_id, payload.get())
                .expect("should allow responding to request")
                .expect("should actually answer request");

            assert_eq!(resp.header().channel(), channel);
            assert_eq!(resp.header().id(), req_id);
            assert_eq!(resp.header().kind(), payload.response_kind());

            // Outgoing set should be empty afterwards.
            assert_eq!(
                protocol
                    .lookup_channel(channel)
                    .expect("should find channel")
                    .incoming_requests,
                [leftover_id].into()
            );
        }
    }

    #[test]
    fn custom_errors_are_possible() {
        let mut protocol = ProtocolBuilder::<4>::new().build();

        // The channel ID for custom errors can be arbitrary!
        let id = Id::new(12345);
        let channel = ChannelId::new(123);
        let outgoing = protocol
            .custom_error(channel, id, Bytes::new())
            .expect("should be able to send custom error");

        assert_eq!(outgoing.header().id(), id);
        assert_eq!(outgoing.header().channel(), channel);
        assert_eq!(outgoing.header().error_kind(), ErrorKind::Other);
    }

    /// A simplified setup for testing back and forth between two peers.
    #[derive(Clone, Debug)]
    struct TestingSetup {
        /// Alice's protocol state.
        alice: JulietProtocol<{ Self::NUM_CHANNELS as usize }>,
        /// Bob's protocol state.
        bob: JulietProtocol<{ Self::NUM_CHANNELS as usize }>,
        /// The channel communication is sent across for these tests.
        common_channel: ChannelId,
        /// Maximum frame size in test environment.
        max_frame_size: MaxFrameSize,
    }

    /// Peer selection.
    ///
    /// Used to select a target when interacting with the test environment.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]

    enum Peer {
        /// Alice.
        Alice,
        /// Bob, aka "not Alice".
        Bob,
    }

    impl Not for Peer {
        type Output = Self;

        fn not(self) -> Self::Output {
            match self {
                Alice => Bob,
                Bob => Alice,
            }
        }
    }

    use Peer::{Alice, Bob};

    impl TestingSetup {
        const MAX_PAYLOAD_SIZE: u32 = 512;
        const MAX_FRAME_SIZE: u32 = 20;
        const NUM_CHANNELS: u8 = 4;

        /// Instantiates a new testing setup.
        fn new() -> Self {
            let max_frame_size = MaxFrameSize::new(Self::MAX_FRAME_SIZE);
            let pb = ProtocolBuilder::with_default_channel_config(
                ChannelConfiguration::new()
                    .with_request_limit(2)
                    .with_max_request_payload_size(Self::MAX_PAYLOAD_SIZE)
                    .with_max_response_payload_size(Self::MAX_PAYLOAD_SIZE),
            )
            .max_frame_size(max_frame_size.get());
            let common_channel = ChannelId(Self::NUM_CHANNELS - 1);

            let alice = pb.build();
            let bob = pb.build();

            TestingSetup {
                alice,
                bob,
                common_channel,
                max_frame_size,
            }
        }

        /// Retrieves a handle to the protocol state of the given peer.
        #[inline]
        fn get_peer_mut(&mut self, peer: Peer) -> &mut JulietProtocol<4> {
            match peer {
                Alice => &mut self.alice,
                Bob => &mut self.bob,
            }
        }

        /// Take `msg` and send it to peer `dest`.
        ///
        /// Will check that the message is fully processed and removed on [`Outcome::Success`].
        fn recv_on(
            &mut self,
            dest: Peer,
            msg: OutgoingMessage,
        ) -> Result<CompletedRead, OutgoingMessage> {
            let msg_bytes = msg.to_bytes(self.max_frame_size);
            let mut msg_bytes_buffer = BytesMut::from(msg_bytes.as_ref());

            let orig_self = self.clone();

            let expected = self
                .get_peer_mut(dest)
                .process_incoming(&mut msg_bytes_buffer)
                .to_result()
                .map(|v| {
                    assert!(
                        msg_bytes_buffer.is_empty(),
                        "client should have consumed input"
                    );
                    v
                });

            // Test parsing of partially received data.
            //
            // This loop runs through almost every sensibly conceivable size of chunks in which data
            // can be transmitted and simulates a trickling reception. The original state of the
            // receiving facilities is cloned first, and the outcome of the trickle reception is
            // compared against the reference of receiving in one go from earlier (`expected`).
            for transmission_chunk_size in 1..=(self.max_frame_size.get() as usize * 2 + 1) {
                let mut unsent = msg_bytes.clone();
                let mut buffer = BytesMut::new();
                let mut this = orig_self.clone();

                let result = loop {
                    // Put more data from unsent into the buffer.
                    let chunk = unsent.split_to(transmission_chunk_size.min(unsent.remaining()));
                    buffer.extend(chunk);

                    let outcome = this.get_peer_mut(dest).process_incoming(&mut buffer);

                    if matches!(outcome, Outcome::Incomplete(_)) {
                        if unsent.is_empty() {
                            panic!(
                                "got incompletion before completion  while attempting to send \
                                message piecewise in {} byte chunks",
                                transmission_chunk_size
                            );
                        }

                        // Continue reading until complete.
                        continue;
                    }

                    break outcome.to_result();
                };

                assert_eq!(result, expected, "should not see difference between trickling reception and single send reception");
            }

            expected
        }

        /// Take `msg` and send it to peer `dest`.
        ///
        /// Will check that the message is fully processed and removed, and a new header read
        /// expected next.
        fn expect_consumes(&mut self, dest: Peer, msg: OutgoingMessage) {
            let mut msg_bytes = BytesMut::from(msg.to_bytes(self.max_frame_size).as_ref());

            let outcome = self.get_peer_mut(dest).process_incoming(&mut msg_bytes);

            assert!(msg_bytes.is_empty(), "client should have consumed input");

            assert_matches!(outcome, Outcome::Incomplete(n) if n.get() == 4);
        }

        /// Creates a new request on peer `origin`, the sends it to the other peer.
        ///
        /// Returns the outcome of the other peer's reception.
        #[track_caller]
        fn create_and_send_request(
            &mut self,
            origin: Peer,
            payload: Option<Bytes>,
        ) -> Result<CompletedRead, OutgoingMessage> {
            let channel = self.common_channel;
            let msg = self
                .get_peer_mut(origin)
                .create_request(channel, payload)
                .expect("should be able to create request");

            self.recv_on(!origin, msg)
        }

        /// Similar to `create_and_send_request`, but bypasses all checks.
        ///
        /// Allows for sending requests that are normally not allowed by the protocol API.
        #[track_caller]
        fn inject_and_send_request(
            &mut self,
            origin: Peer,
            payload: Option<Bytes>,
        ) -> Result<CompletedRead, OutgoingMessage> {
            let channel_id = self.common_channel;
            let origin_channel = self
                .get_peer_mut(origin)
                .lookup_channel_mut(channel_id)
                .expect("channel does not exist, why?");

            // Create request, bypassing all checks usually performed by the protocol.
            let msg = origin_channel.create_unchecked_request(channel_id, payload);

            // Send to peer and return outcome.
            self.recv_on(!origin, msg)
        }

        /// Creates a new request cancellation on peer `origin`, the sends it to the other peer.
        ///
        /// Returns the outcome of the other peer's reception.
        #[track_caller]
        fn cancel_request_and_send(
            &mut self,
            origin: Peer,
            id: Id,
        ) -> Option<Result<CompletedRead, OutgoingMessage>> {
            let channel = self.common_channel;
            let msg = self
                .get_peer_mut(origin)
                .cancel_request(channel, id)
                .expect("should be able to create request cancellation")?;

            Some(self.recv_on(!origin, msg))
        }

        /// Creates a new response cancellation on peer `origin`, the sends it to the other peer.
        ///
        /// Returns the outcome of the other peer's reception.
        #[track_caller]
        fn cancel_response_and_send(
            &mut self,
            origin: Peer,
            id: Id,
        ) -> Option<Result<CompletedRead, OutgoingMessage>> {
            let channel = self.common_channel;
            let msg = self
                .get_peer_mut(origin)
                .cancel_response(channel, id)
                .expect("should be able to create response cancellation")?;

            Some(self.recv_on(!origin, msg))
        }

        /// Creates a new response on peer `origin`, the sends it to the other peer.
        ///
        /// Returns the outcome of the other peer's reception. If no response was scheduled for
        /// sending, returns `None`.
        #[track_caller]
        fn create_and_send_response(
            &mut self,
            origin: Peer,
            id: Id,
            payload: Option<Bytes>,
        ) -> Option<Result<CompletedRead, OutgoingMessage>> {
            let channel = self.common_channel;

            let msg = self
                .get_peer_mut(origin)
                .create_response(channel, id, payload)
                .expect("should be able to create response")?;

            Some(self.recv_on(!origin, msg))
        }

        /// Similar to `create_and_send_response`, but bypasses all checks.
        ///
        /// Allows for sending requests that are normally not allowed by the protocol API.
        #[track_caller]
        fn inject_and_send_response(
            &mut self,
            origin: Peer,
            id: Id,
            payload: Option<Bytes>,
        ) -> Result<CompletedRead, OutgoingMessage> {
            let channel_id = self.common_channel;

            let msg = create_unchecked_response(channel_id, id, payload);

            // Send to peer and return outcome.
            self.recv_on(!origin, msg)
        }

        /// Similar to `create_and_send_response_cancellation`, but bypasses all checks.
        ///
        /// Allows for sending request cancellations that are not allowed by the protocol API.
        #[track_caller]
        fn inject_and_send_response_cancellation(
            &mut self,
            origin: Peer,
            id: Id,
        ) -> Result<CompletedRead, OutgoingMessage> {
            let channel_id = self.common_channel;

            let msg = create_unchecked_response_cancellation(channel_id, id);

            // Send to peer and return outcome.
            self.recv_on(!origin, msg)
        }

        /// Asserts the given completed read is a [`CompletedRead::NewRequest`] with the given ID
        /// and payload.
        ///
        /// # Panics
        ///
        /// Will panic if the assertion fails.
        #[track_caller]
        fn assert_is_new_request(
            &self,
            expected_id: Id,
            expected_payload: Option<&[u8]>,
            completed_read: CompletedRead,
        ) {
            assert_matches!(
                completed_read,
                CompletedRead::NewRequest {
                    channel,
                    id,
                    payload
                } => {
                    assert_eq!(channel, self.common_channel);
                    assert_eq!(id, expected_id);
                    assert_eq!(payload.as_deref(), expected_payload);
                }
            );
        }

        /// Asserts the given completed read is a [`CompletedRead::RequestCancellation`] with the
        /// given ID.
        ///
        /// # Panics
        ///
        /// Will panic if the assertion fails.
        #[track_caller]
        fn assert_is_request_cancellation(&self, expected_id: Id, completed_read: CompletedRead) {
            assert_matches!(
                completed_read,
                CompletedRead::RequestCancellation {
                    channel,
                    id,
                } => {
                    assert_eq!(channel, self.common_channel);
                    assert_eq!(id, expected_id);
                }
            );
        }

        /// Asserts the given completed read is a [`CompletedRead::ReceivedResponse`] with the given
        /// ID and payload.
        ///
        /// # Panics
        ///
        /// Will panic if the assertion fails.
        #[track_caller]
        fn assert_is_received_response(
            &self,
            expected_id: Id,
            expected_payload: Option<&[u8]>,
            completed_read: CompletedRead,
        ) {
            assert_matches!(
                completed_read,
                CompletedRead::ReceivedResponse {
                    channel,
                    id,
                    payload
                } => {
                    assert_eq!(channel, self.common_channel);
                    assert_eq!(id, expected_id);
                    assert_eq!(payload.as_deref(), expected_payload);
                }
            );
        }

        /// Asserts the given completed read is a [`CompletedRead::ResponseCancellation`] with the
        /// given ID.
        ///
        /// # Panics
        ///
        /// Will panic if the assertion fails.
        #[track_caller]
        fn assert_is_response_cancellation(&self, expected_id: Id, completed_read: CompletedRead) {
            assert_matches!(
                completed_read,
                CompletedRead::ResponseCancellation {
                    channel,
                    id,
                } => {
                    assert_eq!(channel, self.common_channel);
                    assert_eq!(id, expected_id);
                }
            );
        }

        /// Asserts given `Result` is of type `Err` and its message contains a specific header.
        ///
        /// # Panics
        ///
        /// Will panic if the assertion fails.
        #[track_caller]
        fn assert_is_error_message<T: Debug>(
            &self,
            error_kind: ErrorKind,
            id: Id,
            result: Result<T, OutgoingMessage>,
        ) {
            let err = result.expect_err("expected an error, got positive outcome instead");
            let header = err.header();
            assert_eq!(header.error_kind(), error_kind);
            assert_eq!(header.id(), id);
            assert_eq!(header.channel(), self.common_channel);
        }
    }

    #[test]
    fn use_case_req_ok() {
        for payload in VaryingPayload::all_valid() {
            let mut env = TestingSetup::new();

            let expected_id = Id::new(1);
            let bob_completed_read = env
                .create_and_send_request(Alice, payload.get())
                .expect("bob should accept request");
            env.assert_is_new_request(expected_id, payload.get_slice(), bob_completed_read);

            // Return a response.
            let alice_completed_read = env
                .create_and_send_response(Bob, expected_id, payload.get())
                .expect("did not expect response to be dropped")
                .expect("should not fail to process response on alice");
            env.assert_is_received_response(expected_id, payload.get_slice(), alice_completed_read);
        }
    }

    // A request followed by a response can take multiple orders, all of which are valid:

    // Alice:Request, Alice:Cancel, Bob:Respond     (cancellation ignored)
    // Alice:Request, Alice:Cancel, Bob:Cancel      (cancellation honored or Bob cancelled)
    // Alice:Request, Bob:Respond, Alice:Cancel     (cancellation not in time)
    // Alice:Request, Bob:Cancel, Alice:Cancel      (cancellation acknowledged)

    // Alice's cancellation can also be on the wire at the same time as Bob's responses.
    // Alice:Request, Bob:Respond, Alice:CancelSim  (cancellation arrives after response)
    // Alice:Request, Bob:Cancel, Alice:CancelSim   (cancellation arrives after cancellation)

    /// Sets up the environment with Alice's initial request.
    fn env_with_initial_areq(payload: VaryingPayload) -> (TestingSetup, Id) {
        let mut env = TestingSetup::new();

        let expected_id = Id::new(1);

        // Alice sends a request first.
        let bob_initial_completed_read = env
            .create_and_send_request(Alice, payload.get())
            .expect("bob should accept request");
        env.assert_is_new_request(expected_id, payload.get_slice(), bob_initial_completed_read);

        (env, expected_id)
    }

    #[test]
    fn use_case_areq_acnc_brsp() {
        // Alice:Request, Alice:Cancel, Bob:Respond
        for payload in VaryingPayload::all_valid() {
            let (mut env, id) = env_with_initial_areq(payload);
            let bob_read_of_cancel = env
                .cancel_request_and_send(Alice, id)
                .expect("alice should send cancellation")
                .expect("bob should produce cancellation");
            env.assert_is_request_cancellation(id, bob_read_of_cancel);

            // Bob's application doesn't notice and sends the response anyway. It should at arrive
            // at Alice's to confirm the cancellation.
            let alices_read = env
                .create_and_send_response(Bob, id, payload.get())
                .expect("bob must send the response")
                .expect("bob should be ablet to create the response");

            env.assert_is_received_response(id, payload.get_slice(), alices_read);
        }
    }

    #[test]
    fn use_case_areq_acnc_bcnc() {
        // Alice:Request, Alice:Cancel, Bob:Respond
        for payload in VaryingPayload::all_valid() {
            let (mut env, id) = env_with_initial_areq(payload);

            // Alice directly follows with a cancellation.
            let bob_read_of_cancel = env
                .cancel_request_and_send(Alice, id)
                .expect("alice should send cancellation")
                .expect("bob should produce cancellation");
            env.assert_is_request_cancellation(id, bob_read_of_cancel);

            // Bob's application confirms with a response cancellation.
            let alices_read = env
                .cancel_response_and_send(Bob, id)
                .expect("bob must send the response")
                .expect("bob should be ablet to create the response");
            env.assert_is_response_cancellation(id, alices_read);
        }
    }

    #[test]
    fn use_case_areq_brsp_acnc() {
        // Alice:Request, Bob:Respond, Alice:Cancel
        for payload in VaryingPayload::all_valid() {
            let (mut env, id) = env_with_initial_areq(payload);

            // Bob's application responds.
            let alices_read = env
                .create_and_send_response(Bob, id, payload.get())
                .expect("bob must send the response")
                .expect("bob should be ablet to create the response");
            env.assert_is_received_response(id, payload.get_slice(), alices_read);

            // Alice's app attempts to send a cancellation, which should be swallowed.
            assert!(env.cancel_request_and_send(Alice, id).is_none());
        }
    }

    #[test]
    fn use_case_areq_bcnc_acnc() {
        // Alice:Request, Bob:Respond, Alice:Cancel
        for payload in VaryingPayload::all_valid() {
            let (mut env, id) = env_with_initial_areq(payload);

            // Bob's application answers with a response cancellation.
            let alices_read = env
                .cancel_response_and_send(Bob, id)
                .expect("bob must send the response")
                .expect("bob should be ablet to create the response");
            env.assert_is_response_cancellation(id, alices_read);

            // Alice's app attempts to send a cancellation, which should be swallowed.
            assert!(env.cancel_request_and_send(Alice, id).is_none());
        }
    }

    #[test]
    fn use_case_areq_brsp_acncsim() {
        // Alice:Request, Bob:Respond, Alice:CancelSim
        for payload in VaryingPayload::all_valid() {
            let (mut env, id) = env_with_initial_areq(payload);

            // Bob's application responds.
            let alices_read = env
                .create_and_send_response(Bob, id, payload.get())
                .expect("bob must send the response")
                .expect("bob should be ablet to create the response");
            env.assert_is_received_response(id, payload.get_slice(), alices_read);

            // Alice's app attempts to send a cancellation due to a race condition.
            env.expect_consumes(
                Bob,
                create_unchecked_request_cancellation(env.common_channel, id),
            );
        }
    }

    #[test]
    fn use_case_areq_bcnc_acncsim() {
        // Alice:Request, Bob:Respond, Alice:CancelSim
        for payload in VaryingPayload::all_valid() {
            let (mut env, id) = env_with_initial_areq(payload);

            // Bob's application cancels.
            let alices_read = env
                .cancel_response_and_send(Bob, id)
                .expect("bob must send the response")
                .expect("bob should be ablet to create the response");

            env.assert_is_response_cancellation(id, alices_read);
            env.expect_consumes(
                Bob,
                create_unchecked_request_cancellation(env.common_channel, id),
            );
        }
    }

    #[test]
    fn env_req_exceed_in_flight_limit() {
        for payload in VaryingPayload::all_valid() {
            let mut env = TestingSetup::new();
            let bob_completed_read_1 = env
                .create_and_send_request(Alice, payload.get())
                .expect("bob should accept request 1");
            env.assert_is_new_request(Id::new(1), payload.get_slice(), bob_completed_read_1);

            let bob_completed_read_2 = env
                .create_and_send_request(Alice, payload.get())
                .expect("bob should accept request 2");
            env.assert_is_new_request(Id::new(2), payload.get_slice(), bob_completed_read_2);

            // We now need to bypass the local protocol checks to inject a malicious one.

            let local_err_result = env.inject_and_send_request(Alice, payload.get());

            env.assert_is_error_message(
                ErrorKind::RequestLimitExceeded,
                Id::new(3),
                local_err_result,
            );
        }
    }

    #[test]
    fn env_req_exceed_req_size_limit() {
        let payload = VaryingPayload::TooLarge;

        let mut env = TestingSetup::new();
        let bob_result = env.inject_and_send_request(Alice, payload.get());

        env.assert_is_error_message(ErrorKind::RequestTooLarge, Id::new(1), bob_result);
    }

    #[test]
    fn env_req_duplicate_request() {
        for payload in VaryingPayload::all_valid() {
            let mut env = TestingSetup::new();

            let bob_completed_read_1 = env
                .create_and_send_request(Alice, payload.get())
                .expect("bob should accept request 1");
            env.assert_is_new_request(Id::new(1), payload.get_slice(), bob_completed_read_1);

            // Send a second request with the same ID. For this, we manipulate Alice's internal
            // counter and state.
            let alice_channel = env
                .alice
                .lookup_channel_mut(env.common_channel)
                .expect("should have channel");
            alice_channel.prev_request_id -= 1;
            alice_channel.outgoing_requests.clear();

            let second_send_result = env.inject_and_send_request(Alice, payload.get());
            env.assert_is_error_message(
                ErrorKind::DuplicateRequest,
                Id::new(1),
                second_send_result,
            );
        }
    }

    #[test]
    fn env_req_response_for_ficticious_request() {
        for payload in VaryingPayload::all_valid() {
            let mut env = TestingSetup::new();

            let bob_completed_read_1 = env
                .create_and_send_request(Alice, payload.get())
                .expect("bob should accept request 1");
            env.assert_is_new_request(Id::new(1), payload.get_slice(), bob_completed_read_1);

            // Send a response with a wrong ID.
            let second_send_result = env.inject_and_send_response(Bob, Id::new(123), payload.get());
            env.assert_is_error_message(
                ErrorKind::FictitiousRequest,
                Id::new(123),
                second_send_result,
            );
        }
    }

    #[test]
    fn env_req_cancellation_for_ficticious_request() {
        for payload in VaryingPayload::all_valid() {
            let mut env = TestingSetup::new();

            let bob_completed_read_1 = env
                .create_and_send_request(Alice, payload.get())
                .expect("bob should accept request 1");
            env.assert_is_new_request(Id::new(1), payload.get_slice(), bob_completed_read_1);

            // Have bob send a response for a request that was never made.
            let alice_result = env.inject_and_send_response(Bob, Id::new(123), payload.get());
            env.assert_is_error_message(ErrorKind::FictitiousRequest, Id::new(123), alice_result);
        }
    }

    #[test]
    fn env_req_size_limit_exceeded() {
        let mut env = TestingSetup::new();

        let payload = VaryingPayload::TooLarge;

        // Alice should not allow too-large requests to be sent.
        let violation = env
            .alice
            .create_request(env.common_channel, payload.get())
            .expect_err("should not be able to create too large request");

        assert_matches!(violation, LocalProtocolViolation::PayloadExceedsLimit);

        // If we force the issue, Bob must refuse it instead.
        let bob_result = env.inject_and_send_request(Alice, payload.get());
        env.assert_is_error_message(ErrorKind::RequestTooLarge, Id::new(1), bob_result);
    }

    #[test]
    fn env_response_size_limit_exceeded() {
        let (mut env, id) = env_with_initial_areq(VaryingPayload::None);
        let payload = VaryingPayload::TooLarge;

        // Bob should not allow too-large responses to be sent.
        let violation = env
            .bob
            .create_request(env.common_channel, payload.get())
            .expect_err("should not be able to create too large response");
        assert_matches!(violation, LocalProtocolViolation::PayloadExceedsLimit);

        // If we force the issue, Alice must refuse it.
        let alice_result = env.inject_and_send_response(Bob, id, payload.get());
        env.assert_is_error_message(ErrorKind::ResponseTooLarge, Id::new(1), alice_result);
    }

    #[test]
    fn env_req_response_cancellation_limit_exceeded() {
        for payload in VaryingPayload::all_valid() {
            for num_requests in 0..=2 {
                let mut env = TestingSetup::new();

                // Have Alice make requests in order to fill-up the in-flights.
                for i in 0..num_requests {
                    let expected_id = Id::new(i + 1);
                    let bobs_read = env
                        .create_and_send_request(Alice, payload.get())
                        .expect("should accept request");
                    env.assert_is_new_request(expected_id, payload.get_slice(), bobs_read);
                }

                // Now send the corresponding amount of cancellations.
                for i in 0..num_requests {
                    let id = Id::new(i + 1);

                    let msg = create_unchecked_request_cancellation(env.common_channel, id);

                    let bobs_read = env.recv_on(Bob, msg).expect("cancellation should not fail");
                    env.assert_is_request_cancellation(id, bobs_read);
                }

                let id = Id::new(num_requests + 1);
                // Finally another cancellation should trigger an error.
                let msg = create_unchecked_request_cancellation(env.common_channel, id);

                let bobs_result = env.recv_on(Bob, msg);
                env.assert_is_error_message(ErrorKind::CancellationLimitExceeded, id, bobs_result);
            }
        }
    }

    #[test]
    fn env_max_frame_size_exceeded() {
        // Note: An actual `MaxFrameSizeExceeded` can never occur due to how this library is
        //       implemented. This is the closest situation that can occur.

        let mut env = TestingSetup::new();

        let payload = VaryingPayload::TooLarge;
        let id = Id::new(1);

        // We have to craft the message by hand to exceed the frame size.
        let msg = OutgoingMessage::new(
            Header::new(Kind::RequestPl, env.common_channel, id),
            payload.get(),
        );
        let mut encoded = BytesMut::from(
            msg.to_bytes(MaxFrameSize::new(
                2 * payload
                    .get()
                    .expect("TooLarge payload should have body")
                    .len() as u32,
            ))
            .as_ref(),
        );
        let violation = env.bob.process_incoming(&mut encoded).to_result();

        env.assert_is_error_message(ErrorKind::RequestTooLarge, id, violation);
    }

    #[test]
    fn env_invalid_header() {
        for payload in VaryingPayload::all_valid() {
            let mut env = TestingSetup::new();

            let id = Id::new(123);

            // We have to craft the message by hand to exceed the frame size.
            let msg = OutgoingMessage::new(
                Header::new(Kind::RequestPl, env.common_channel, id),
                payload.get(),
            );
            let mut encoded = BytesMut::from(msg.to_bytes(env.max_frame_size).as_ref());

            // Patch the header so that it is broken.
            encoded[0] = 0b0000_1111; // Kind: Normal, all data bits set.

            let violation = env
                .bob
                .process_incoming(&mut encoded)
                .to_result()
                .expect_err("expected invalid header to produce an error");

            // We have to manually assert the error, since invalid header errors are sent with an ID
            // of 0 and on channel 0.

            let header = violation.header();
            assert_eq!(header.error_kind(), ErrorKind::InvalidHeader);
            assert_eq!(header.id(), Id::new(0));
            assert_eq!(header.channel(), ChannelId::new(0));
        }
    }

    #[test]
    fn env_bad_varint() {
        let payload = VaryingPayload::MultiFrame;
        let mut env = TestingSetup::new();

        let id = Id::new(1);

        // We have to craft the message by hand to exceed the frame size.
        let msg = OutgoingMessage::new(
            Header::new(Kind::RequestPl, env.common_channel, id),
            payload.get(),
        );
        let mut encoded = BytesMut::from(msg.to_bytes(env.max_frame_size).as_ref());

        // Invalidate the varint.
        encoded[4] = 0xFF;
        encoded[5] = 0xFF;
        encoded[6] = 0xFF;
        encoded[7] = 0xFF;
        encoded[8] = 0xFF;

        let violation = env.bob.process_incoming(&mut encoded).to_result();

        env.assert_is_error_message(ErrorKind::BadVarInt, id, violation);
    }

    #[test]
    fn response_with_no_payload_is_cleared_from_buffer() {
        // This test is fairly specific from a concrete bug. In general, buffer advancement is
        // tested in other tests as one of many condition checks.

        let mut protocol: JulietProtocol<16> = ProtocolBuilder::with_default_channel_config(
            ChannelConfiguration::new()
                .with_max_request_payload_size(4096)
                .with_max_response_payload_size(4096),
        )
        .build();

        let channel = ChannelId::new(6);
        let id = Id::new(1);

        // Create the request to prime the protocol state machine for the incoming response.
        let msg = protocol
            .create_request(channel, Some(Bytes::from(&b"foobar"[..])))
            .expect("can create request");

        assert_eq!(msg.header().channel(), channel);
        assert_eq!(msg.header().id(), id);

        let mut response_raw =
            BytesMut::from(&Header::new(Kind::Response, channel, id).as_ref()[..]);

        assert_eq!(response_raw.remaining(), 4);

        let outcome = protocol
            .process_incoming(&mut response_raw)
            .expect("should complete outcome");
        assert_eq!(
            outcome,
            CompletedRead::ReceivedResponse {
                channel,
                /// The ID of the request received.
                id,
                /// The response payload.
                payload: None,
            }
        );

        assert_eq!(response_raw.remaining(), 0);
    }

    #[test]
    fn one_respone_or_cancellation_per_request() {
        for payload in VaryingPayload::all_valid() {
            // Case 1: Response, response.
            let (mut env, id) = env_with_initial_areq(payload);
            let completed_read = env
                .create_and_send_response(Bob, id, payload.get())
                .expect("should send response")
                .expect("should accept response");
            env.assert_is_received_response(id, payload.get_slice(), completed_read);

            let alice_result = env.inject_and_send_response(Bob, id, payload.get());
            env.assert_is_error_message(ErrorKind::FictitiousRequest, id, alice_result);

            // Case 2: Response, cancel.
            let (mut env, id) = env_with_initial_areq(payload);
            let completed_read = env
                .create_and_send_response(Bob, id, payload.get())
                .expect("should send response")
                .expect("should accept response");
            env.assert_is_received_response(id, payload.get_slice(), completed_read);

            let alice_result = env.inject_and_send_response_cancellation(Bob, id);
            env.assert_is_error_message(ErrorKind::FictitiousCancel, id, alice_result);

            // Case 3: Cancel, response.
            let (mut env, id) = env_with_initial_areq(payload);
            let completed_read = env
                .cancel_response_and_send(Bob, id)
                .expect("should send response cancellation")
                .expect("should accept response cancellation");
            env.assert_is_response_cancellation(id, completed_read);

            let alice_result = env.inject_and_send_response(Bob, id, payload.get());
            env.assert_is_error_message(ErrorKind::FictitiousRequest, id, alice_result);

            // Case4: Cancel, cancel.
            let (mut env, id) = env_with_initial_areq(payload);
            let completed_read = env
                .create_and_send_response(Bob, id, payload.get())
                .expect("should send response")
                .expect("should accept response");
            env.assert_is_received_response(id, payload.get_slice(), completed_read);

            let alice_result = env.inject_and_send_response(Bob, id, payload.get());
            env.assert_is_error_message(ErrorKind::FictitiousRequest, id, alice_result);
        }
    }

    #[test]
    fn multiframe_messages_cancelled_correctly_after_partial_reception() {
        // We send a single frame of a multi-frame payload.
        let payload = VaryingPayload::MultiFrame;

        let mut env = TestingSetup::new();

        let expected_id = Id::new(1);
        let channel = env.common_channel;

        // Alice sends a multi-frame request.
        let alices_multiframe_request = env
            .get_peer_mut(Alice)
            .create_request(channel, payload.get())
            .expect("should be able to create request");
        let req_header = alices_multiframe_request.header();

        assert!(alices_multiframe_request.is_multi_frame(env.max_frame_size));

        let frames = alices_multiframe_request.frames();
        let (frame, _additional_frames) = frames.next_owned(env.max_frame_size);
        let mut buffer = BytesMut::from(frame.to_bytes().as_ref());

        // The outcome of receiving a single frame should be a begun multi-frame read and 4 bytes
        // incompletion asking for the next header.
        let outcome = env.get_peer_mut(Bob).process_incoming(&mut buffer);
        assert_eq!(outcome, Outcome::incomplete(4));

        let bobs_channel = &env.get_peer_mut(Bob).channels[channel.get() as usize];
        let mut expected = HashSet::new();
        expected.insert(expected_id);
        assert_eq!(bobs_channel.incoming_requests, expected);
        assert!(matches!(
            bobs_channel.current_multiframe_receiver,
            MultiframeReceiver::InProgress {
                header,
                ..
            } if header == req_header
        ));

        // Now send the cancellation.
        let cancellation_frames = env
            .get_peer_mut(Alice)
            .cancel_request(channel, expected_id)
            .expect("alice should be able to create the cancellation")
            .expect("should required to send cancellation")
            .frames();
        let (cancellation_frame, _additional_frames) =
            cancellation_frames.next_owned(env.max_frame_size);
        let mut buffer = BytesMut::from(cancellation_frame.to_bytes().as_ref());

        let bobs_outcome = env.get_peer_mut(Bob).process_incoming(&mut buffer);

        // Processing the cancellation should have no external effect.
        assert_eq!(bobs_outcome, Outcome::incomplete(4));

        // Finally, check if the state is as expected. Since it is an incomplete multi-channel
        // message, we must cancel the transfer early.
        let bobs_channel = &env.get_peer_mut(Bob).channels[channel.get() as usize];

        assert!(bobs_channel.incoming_requests.is_empty());
        assert!(matches!(
            bobs_channel.current_multiframe_receiver,
            MultiframeReceiver::Ready
        ));
    }
}
