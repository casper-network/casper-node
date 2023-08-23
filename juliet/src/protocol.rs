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
#[derive(Copy, Clone, Debug, Error)]
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

        // The `unwrap_or` below should never be triggered, as long as `u16::MAX` or less
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

        let header = Header::new(header::Kind::CancelResp, channel, id);
        Ok(Some(OutgoingMessage::new(header, None)))
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
    use std::collections::HashSet;

    use bytes::{Buf, Bytes, BytesMut};
    use proptest_attr_macro::proptest;
    use strum::IntoEnumIterator;

    use crate::{
        header::{ErrorKind, Header, Kind},
        protocol::{payload_is_multi_frame, CompletedRead, LocalProtocolViolation},
        ChannelConfiguration, ChannelId, Id, Outcome,
    };

    use super::{err_msg, Channel, JulietProtocol, MaxFrameSize, ProtocolBuilder};

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
        const LONG_PAYLOAD: &[u8] =
            b"large payload large payload large payload large payload large payload large payload";

        // Try different payload sizes (no payload, single frame payload, multiframe payload).
        for payload in [
            None,
            Some(Bytes::from_static(b"asdf")),
            Some(Bytes::from_static(LONG_PAYLOAD)),
        ] {
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
            let expected_header_kind = if payload.is_none() {
                Kind::Request
            } else {
                Kind::RequestPl
            };

            let req = protocol
                .create_request(channel, payload)
                .expect("should be able to create request");

            assert_eq!(req.header().channel(), channel);
            assert_eq!(req.header().kind(), expected_header_kind);

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
                .create_request(other_channel, None)
                .expect("should be able to create request");

            assert_eq!(other_req.header().channel(), other_channel);
            assert_eq!(other_req.header().kind(), Kind::Request);

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
        // Configure a protocol with payload, at least 10 bytes segment size.
        let mut protocol = ProtocolBuilder::<2>::new().build();

        let channel = ChannelId::new(1);

        // Try an invalid channel, should result in an error.
        assert!(matches!(
            protocol.create_request(ChannelId::new(2), None),
            Err(LocalProtocolViolation::InvalidChannel(ChannelId(2)))
        ));

        assert!(protocol
            .allowed_to_send_request(channel)
            .expect("channel should exist"));
        let _ = protocol
            .create_request(channel, None)
            .expect("should be able to create request");

        assert!(matches!(
            protocol.create_request(channel, None),
            Err(LocalProtocolViolation::WouldExceedRequestLimit)
        ));
    }

    #[test]
    fn create_response_with_correct_input_clears_state_accordingly() {
        let mut protocol = ProtocolBuilder::<4>::new().build();

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
            .create_response(channel, Id::new(12), None)
            .expect("should allow attempting to respond to non-existent request")
            .is_none());

        // Actual response.
        let resp = protocol
            .create_response(channel, req_id, None)
            .expect("should allow responding to request")
            .expect("should actually answer request");

        assert_eq!(resp.header().channel(), channel);
        assert_eq!(resp.header().id(), req_id);
        assert_eq!(resp.header().kind(), Kind::Response);

        // Outgoing set should be empty afterwards.
        assert_eq!(
            protocol
                .lookup_channel(channel)
                .expect("should find channel")
                .incoming_requests,
            [leftover_id].into()
        );
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

    #[test]
    fn use_case_send_request_with_no_payload() {
        todo!("simulate a working request that sends a single request with no payload, should produce appropriate events on receiving side, using transmissions inputs");
    }

    #[test]
    fn model_based_single_roundtrip_test() {
        todo!("model a single request interaction with various outcomes and test across various transmission stutter steps");
    }

    #[test]
    fn error_codes_set_appropriately_on_request_reception() {
        todo!("sending invalid requests should produce the appropriate errors")
    }

    #[test]
    fn error_codes_set_appropriately_on_response_reception() {
        todo!("sending invalid responses should produce the appropriate errors")
    }

    #[test]
    fn exceeding_cancellation_allowance_triggers_error() {
        todo!("should not be possible to exceed the cancellation allowance")
    }

    #[test]
    fn cancelling_requests_clears_state_and_causes_dropping_of_outbound_replies() {
        todo!("if a cancellation for a request is received, the outbound response should be cancelled, and a cancellation produced as well")
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
                channel: channel,
                /// The ID of the request received.
                id: id,
                /// The response payload.
                payload: None,
            }
        );

        assert_eq!(response_raw.remaining(), 0);
    }
}
