use std::{
    fmt::{self, Debug, Display, Formatter},
    net::SocketAddr,
    sync::Arc,
};

use futures::future::BoxFuture;
use juliet::ChannelId;
use serde::{
    de::{DeserializeOwned, Error as SerdeError},
    Deserialize, Deserializer, Serialize, Serializer,
};
use strum::{Display, EnumCount, EnumDiscriminants, EnumIter, FromRepr};

use casper_hashing::Digest;
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{crypto, AsymmetricType, ProtocolVersion, PublicKey, SecretKey, Signature};

use super::{connection_id::ConnectionId, serialize_network_message, Ticket};
use crate::{
    effect::EffectBuilder,
    protocol,
    types::{Chainspec, NodeId},
    utils::{
        opt_display::OptDisplay,
        specimen::{
            largest_get_sync_request, largest_get_sync_response, largest_get_unsync_request,
            largest_get_unsync_response, Cache, LargestSpecimen, SizeEstimator,
        },
    },
};

/// The default protocol version to use in absence of one in the protocol version field.
#[inline]
fn default_protocol_version() -> ProtocolVersion {
    ProtocolVersion::V1_0_0
}

#[derive(Clone, Debug, Deserialize, Serialize, EnumDiscriminants)]
#[strum_discriminants(derive(strum::EnumIter))]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Message<P> {
    Handshake {
        /// Network we are connected to.
        network_name: String,
        /// The public address of the node connecting.
        public_addr: SocketAddr,
        /// Protocol version the node is speaking.
        #[serde(default = "default_protocol_version")]
        protocol_version: ProtocolVersion,
        /// A self-signed certificate indicating validator status.
        #[serde(default)]
        consensus_certificate: Option<ConsensusCertificate>,
        /// Hash of the chainspec the node is running.
        #[serde(default)]
        chainspec_hash: Option<Digest>,
    },
    Payload(P),
}

impl<P: Payload> Message<P> {
    /// Classifies a message based on its payload.
    #[inline]
    #[allow(dead_code)] // TODO: Re-add, once decision is made whether to keep message classses.
    pub(super) fn classify(&self) -> MessageKind {
        match self {
            Message::Handshake { .. } => MessageKind::Protocol,
            Message::Payload(payload) => payload.message_kind(),
        }
    }

    /// Determines whether or not a message is low priority.
    #[inline]
    pub(super) fn is_low_priority(&self) -> bool {
        match self {
            Message::Handshake { .. } => false,
            Message::Payload(payload) => payload.is_low_priority(),
        }
    }

    /// Determine which channel this message should be sent on.
    pub(super) fn get_channel(&self) -> Channel {
        match self {
            Message::Handshake { .. } => Channel::Network,
            Message::Payload(payload) => payload.get_channel(),
        }
    }
}

/// A pair of secret keys used by consensus.
pub(crate) struct NodeKeyPair {
    secret_key: Arc<SecretKey>,
    public_key: PublicKey,
}

impl NodeKeyPair {
    /// Creates a new key pair for consensus signing.
    pub(super) fn new(key_pair: (Arc<SecretKey>, PublicKey)) -> Self {
        Self {
            secret_key: key_pair.0,
            public_key: key_pair.1,
        }
    }

    /// Sign a value using this keypair.
    fn sign<T: AsRef<[u8]>>(&self, value: T) -> Signature {
        crypto::sign(value, &self.secret_key, &self.public_key)
    }

    /// Returns a reference to the public key of this key pair.
    pub(super) fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
}

/// Certificate used to indicate that the peer is a validator using the specified public key.
///
/// Note that this type has custom `Serialize` and `Deserialize` implementations to allow the
/// `public_key` and `signature` fields to be encoded to all-lowercase hex, hence circumventing the
/// checksummed-hex encoding used by `PublicKey` and `Signature` in versions 1.4.2 and 1.4.3.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConsensusCertificate {
    public_key: PublicKey,
    signature: Signature,
}

impl ConsensusCertificate {
    /// Creates a new consensus certificate from a connection ID and key pair.
    pub(super) fn create(connection_id: ConnectionId, key_pair: &NodeKeyPair) -> Self {
        let signature = key_pair.sign(connection_id.as_bytes());
        ConsensusCertificate {
            public_key: key_pair.public_key.clone(),
            signature,
        }
    }

    /// Validates a certificate, returning a `PublicKey` if valid.
    pub(super) fn validate(self, connection_id: ConnectionId) -> Result<PublicKey, crypto::Error> {
        crypto::verify(connection_id.as_bytes(), &self.signature, &self.public_key)?;
        Ok(self.public_key)
    }

    /// Creates a random `ConnectionId`.
    #[cfg(test)]
    fn random(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        let public_key = PublicKey::from(&secret_key);
        ConsensusCertificate::create(
            ConnectionId::random(rng),
            &NodeKeyPair::new((Arc::new(secret_key), public_key)),
        )
    }
}

impl Display for ConsensusCertificate {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "key:{}", self.public_key)
    }
}

/// This type and the `NonHumanReadableCertificate` are helper structs only used in the `Serialize`
/// and `Deserialize` implementations of `ConsensusCertificate` to allow handshaking between nodes
/// running the casper-node v1.4.2 and v1.4.3 software versions.
///
/// Checksummed-hex encoding was introduced in 1.4.2 and was applied to `PublicKey` and `Signature`
/// types, affecting the encoding of `ConsensusCertificate` since handshaking uses a human-readable
/// type of encoder/decoder.
///
/// The 1.4.3 version immediately after 1.4.2 used a slightly different style of checksummed-hex
/// encoding which is incompatible with the 1.4.2 style.  To effectively disable checksummed-hex
/// encoding, we need to use an all-lowercase form of hex encoding for the `PublicKey` and
/// `Signature` types.
///
/// The `HumanReadableCertificate` enables that by explicitly being constructed from all-lowercase
/// hex encoded types, while the `NonHumanReadableCertificate` is a simple mirror of
/// `ConsensusCertificate` to allow us to derive `Serialize` and `Deserialize`, avoiding complex
/// hand-written implementations for the non-human-readable case.
#[derive(Serialize, Deserialize)]
struct HumanReadableCertificate {
    public_key: String,
    signature: String,
}

#[derive(Serialize, Deserialize)]
struct NonHumanReadableCertificate {
    public_key: PublicKey,
    signature: Signature,
}

impl Serialize for ConsensusCertificate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            let human_readable_certificate = HumanReadableCertificate {
                public_key: self.public_key.to_hex().to_lowercase(),
                signature: self.signature.to_hex().to_lowercase(),
            };

            return human_readable_certificate.serialize(serializer);
        }

        let non_human_readable_certificate = NonHumanReadableCertificate {
            public_key: self.public_key.clone(),
            signature: self.signature,
        };
        non_human_readable_certificate.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConsensusCertificate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let human_readable_certificate = HumanReadableCertificate::deserialize(deserializer)?;
            let public_key = PublicKey::from_hex(
                human_readable_certificate
                    .public_key
                    .to_lowercase()
                    .as_bytes(),
            )
            .map_err(D::Error::custom)?;
            let signature = Signature::from_hex(
                human_readable_certificate
                    .signature
                    .to_lowercase()
                    .as_bytes(),
            )
            .map_err(D::Error::custom)?;
            return Ok(ConsensusCertificate {
                public_key,
                signature,
            });
        }

        let non_human_readable_certificate =
            NonHumanReadableCertificate::deserialize(deserializer)?;
        Ok(ConsensusCertificate {
            public_key: non_human_readable_certificate.public_key,
            signature: non_human_readable_certificate.signature,
        })
    }
}

impl<P: Display> Display for Message<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Handshake {
                network_name,
                public_addr,
                protocol_version,
                consensus_certificate,
                chainspec_hash,
            } => {
                write!(
                    f,
                    "handshake: {}, public addr: {}, protocol_version: {}, consensus_certificate: {}, chainspec_hash: {}",
                    network_name,
                    public_addr,
                    protocol_version,
                    OptDisplay::new(consensus_certificate.as_ref(), "none"),

                    OptDisplay::new(chainspec_hash.as_ref(), "none")
                )
            }
            Message::Payload(payload) => write!(f, "payload: {}", payload),
        }
    }
}

/// A classification system for networking messages.
#[derive(Copy, Clone, Debug)]
#[allow(dead_code)] // TODO: Re-add, once decision is made whether or not to keep message classses.
pub(crate) enum MessageKind {
    /// Non-payload messages, like handshakes.
    Protocol,
    /// Messages directly related to consensus.
    Consensus,
    /// Deploys being gossiped.
    DeployGossip,
    /// Blocks being gossiped.
    BlockGossip,
    /// Finality signatures being gossiped.
    FinalitySignatureGossip,
    /// Addresses being gossiped.
    AddressGossip,
    /// Deploys being transferred directly (via requests).
    DeployTransfer,
    /// Blocks for finality signatures being transferred directly (via requests and other means).
    BlockTransfer,
    /// Tries transferred, usually as part of chain syncing.
    TrieTransfer,
    /// Any other kind of payload (or missing classification).
    Other,
}

impl Display for MessageKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MessageKind::Protocol => f.write_str("protocol"),
            MessageKind::Consensus => f.write_str("consensus"),
            MessageKind::DeployGossip => f.write_str("deploy_gossip"),
            MessageKind::BlockGossip => f.write_str("block_gossip"),
            MessageKind::FinalitySignatureGossip => f.write_str("finality_signature_gossip"),
            MessageKind::AddressGossip => f.write_str("address_gossip"),
            MessageKind::DeployTransfer => f.write_str("deploy_transfer"),
            MessageKind::BlockTransfer => f.write_str("block_transfer"),
            MessageKind::TrieTransfer => f.write_str("trie_transfer"),
            MessageKind::Other => f.write_str("other"),
        }
    }
}

/// Multiplexed channel identifier used across a single connection.
///
/// Channels are separated mainly to avoid deadlocking issues where two nodes requests a large
/// amount of items from each other simultaneously, with responses being queued behind requests,
/// whilst the latter are buffered due to backpressure.
///
/// Further separation is done to improve quality of service of certain subsystems, e.g. to
/// guarantee that consensus is not impaired by the transfer of large trie nodes.
#[derive(
    Copy, Clone, Debug, Display, Eq, EnumCount, EnumIter, FromRepr, PartialEq, Ord, PartialOrd,
)]
#[repr(u8)]
pub enum Channel {
    /// Networking layer messages, handshakes and ping/pong.
    Network = 0,
    /// Data solely used for syncing being requested.
    ///
    /// We separate sync data (e.g. trie nodes) requests from regular ("data") requests since the
    /// former are not required for a validating node to make progress on consensus, thus
    /// separating these can improve latency.
    SyncDataRequests = 1,
    /// Sync data requests being answered.
    ///
    /// Responses are separated from requests to ensure liveness (see [`Channel`] documentation).
    SyncDataResponses = 2,
    /// Requests for data used during regular validator operation.
    DataRequests = 3,
    /// Responses for data used during regular validator operation.
    DataResponses = 4,
    /// Consensus-level messages, like finality signature announcements and consensus messages.
    Consensus = 5,
    /// Regular gossip announcements and responses (e.g. for deploys and blocks).
    BulkGossip = 6,
}

impl Channel {
    #[inline(always)]
    pub(crate) fn into_channel_id(self) -> ChannelId {
        ChannelId::new(self as u8)
    }
}

/// Network message payload.
///
/// Payloads are what is transferred across the network outside of control messages from the
/// networking component itself.
pub(crate) trait Payload:
    Serialize + DeserializeOwned + Clone + Debug + Display + Send + Sync + Unpin + 'static
{
    /// Classifies the payload based on its contents.
    fn message_kind(&self) -> MessageKind;

    /// Determines if the payload should be considered low priority.
    fn is_low_priority(&self) -> bool {
        false
    }

    /// Determine which channel a message is supposed to sent/received on.
    fn get_channel(&self) -> Channel;
}

/// Network message conversion support.
pub(crate) trait FromIncoming<P> {
    /// Creates a new value from a received payload.
    fn from_incoming(sender: NodeId, payload: P, ticket: Ticket) -> Self;

    /// Tries to convert a payload into a demand.
    ///
    /// This function can optionally be called before `from_incoming` to attempt to convert an
    /// incoming payload into a potential demand.

    // TODO: Replace both this and `from_incoming` with a single function that returns an
    //       appropriate `Either`.
    fn try_demand_from_incoming(
        _effect_builder: EffectBuilder<Self>,
        _sender: NodeId,
        payload: P,
    ) -> Result<(Self, BoxFuture<'static, Option<P>>), P>
    where
        Self: Sized + Send,
    {
        Err(payload)
    }
}

mod specimen_support {
    use std::iter;

    use serde::Serialize;

    use crate::utils::specimen::{
        largest_variant, Cache, LargestSpecimen, SizeEstimator, HIGHEST_UNICODE_CODEPOINT,
    };

    use super::{ConsensusCertificate, Message, MessageDiscriminants};

    impl<P> LargestSpecimen for Message<P>
    where
        P: Serialize + LargestSpecimen,
    {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            let largest_network_name = estimator.parameter("network_name_limit");

            largest_variant::<Self, MessageDiscriminants, _, _>(
                estimator,
                |variant| match variant {
                    MessageDiscriminants::Handshake => Message::Handshake {
                        network_name: iter::repeat(HIGHEST_UNICODE_CODEPOINT)
                            .take(largest_network_name)
                            .collect(),
                        public_addr: LargestSpecimen::largest_specimen(estimator, cache),
                        protocol_version: LargestSpecimen::largest_specimen(estimator, cache),
                        consensus_certificate: LargestSpecimen::largest_specimen(estimator, cache),
                        chainspec_hash: LargestSpecimen::largest_specimen(estimator, cache),
                    },
                    MessageDiscriminants::Payload => {
                        Message::Payload(LargestSpecimen::largest_specimen(estimator, cache))
                    }
                },
            )
        }
    }

    impl LargestSpecimen for ConsensusCertificate {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            ConsensusCertificate {
                public_key: LargestSpecimen::largest_specimen(estimator, cache),
                signature: LargestSpecimen::largest_specimen(estimator, cache),
            }
        }
    }
}

/// An estimator that uses the serialized network representation as a measure of size.
#[derive(Clone, Debug)]
pub(crate) struct NetworkMessageEstimator<'a> {
    /// The chainspec to retrieve estimation values from.
    chainspec: &'a Chainspec,
}

impl<'a> NetworkMessageEstimator<'a> {
    /// Creates a new network message estimator.
    pub(crate) fn new(chainspec: &'a Chainspec) -> Self {
        Self { chainspec }
    }

    /// Returns a parameter by name as `i64`.
    fn get_parameter(&self, name: &'static str) -> Option<i64> {
        Some(match name {
            // The name limit will be larger than the actual name, so it is a safe upper bound.
            "network_name_limit" => self.chainspec.network_config.name.len() as i64,
            // These limits are making deploys bigger than they actually are, since many items
            // have both a `contract_name` and an `entry_point`. We accept 2X as an upper bound.
            "contract_name_limit" => self.chainspec.deploy_config.max_deploy_size as i64,
            "entry_point_limit" => self.chainspec.deploy_config.max_deploy_size as i64,
            "recent_era_count" => {
                (self.chainspec.core_config.unbonding_delay
                    - self.chainspec.core_config.auction_delay) as i64
            }
            "validator_count" => self.chainspec.core_config.validator_slots as i64,
            "minimum_era_height" => self.chainspec.core_config.minimum_era_height as i64,
            "era_duration_ms" => self.chainspec.core_config.era_duration.millis() as i64,
            "minimum_round_length_ms" => self
                .chainspec
                .core_config
                .minimum_block_time
                .millis()
                .max(1) as i64,
            "max_deploy_size" => self.chainspec.deploy_config.max_deploy_size as i64,
            "approvals_hashes" => {
                (self.chainspec.deploy_config.block_max_deploy_count
                    + self.chainspec.deploy_config.block_max_transfer_count) as i64
            }
            "max_deploys_per_block" => self.chainspec.deploy_config.block_max_deploy_count as i64,
            "max_transfers_per_block" => {
                self.chainspec.deploy_config.block_max_transfer_count as i64
            }
            "average_approvals_per_deploy_in_block" => {
                let max_total_deploys = (self.chainspec.deploy_config.block_max_deploy_count
                    + self.chainspec.deploy_config.block_max_transfer_count)
                    as i64;

                // Note: The +1 is to overestimate, as depending on the serialization format chosen,
                //       spreading out the approvals can increase or decrease the size. For
                //       example, in a length-prefixed encoding, putting them all in one may result
                //       in a smaller size if variable size integer encoding it used. In a format
                //       using separators without trailing separators (e.g. commas in JSON),
                //       spreading out will reduce the total number of bytes.
                ((self.chainspec.deploy_config.block_max_approval_count as i64 + max_total_deploys
                    - 1)
                    / max_total_deploys)
                    .max(0)
                    + 1
            }
            "max_accusations_per_block" => self.chainspec.core_config.validator_slots as i64,
            // `RADIX` from EE.
            "max_pointer_per_node" => 255,
            // Endorsements are currently hard-disabled (via code). If ever re-enabled, this
            // parameter should ideally be removed entirely.
            "endorsements_enabled" => 0,
            _ => return None,
        })
    }
}

/// Creates a serialized specimen of the largest possible networking message.
pub(crate) fn generate_largest_message(
    chainspec: &Chainspec,
    channel: Channel,
) -> Message<protocol::Message> {
    use protocol::Message::*;

    let estimator = &NetworkMessageEstimator::new(chainspec);
    let cache = &mut Cache::default();

    Message::Payload(match channel {
        // Dummy value because the channel isn't used at the moment:
        Channel::Network => GetResponse {
            tag: crate::components::fetcher::Tag::Deploy,
            serialized_item: Arc::new([]),
        },
        Channel::SyncDataRequests => largest_get_sync_request(estimator, cache),
        Channel::SyncDataResponses => largest_get_sync_response(estimator, cache),
        Channel::DataRequests => largest_get_unsync_request(estimator, cache),
        Channel::DataResponses => largest_get_unsync_response(estimator, cache),
        Channel::Consensus => [
            Consensus(LargestSpecimen::largest_specimen(estimator, cache)),
            ConsensusRequest(LargestSpecimen::largest_specimen(estimator, cache)),
            FinalitySignature(LargestSpecimen::largest_specimen(estimator, cache)),
        ]
        .into_iter()
        .max_by_key(|msg| estimator.estimate(msg))
        .unwrap(),
        Channel::BulkGossip => [
            DeployGossiper(LargestSpecimen::largest_specimen(estimator, cache)),
            AddressGossiper(LargestSpecimen::largest_specimen(estimator, cache)),
            BlockGossiper(LargestSpecimen::largest_specimen(estimator, cache)),
            FinalitySignatureGossiper(LargestSpecimen::largest_specimen(estimator, cache)),
        ]
        .into_iter()
        .max_by_key(|msg| estimator.estimate(msg))
        .unwrap(),
    })
}

pub(crate) fn generate_largest_serialized_message(
    chainspec: &Chainspec,
    channel: Channel,
) -> Vec<u8> {
    serialize_network_message(&generate_largest_message(chainspec, channel))
        .expect("did not expect serialization to fail") // it would fail in `SizeEstimator` before failing here
        .into()
}

impl<'a> SizeEstimator for NetworkMessageEstimator<'a> {
    fn estimate<T: Serialize>(&self, val: &T) -> usize {
        serialize_network_message(&val)
            .expect("could not serialize given item with network encoding")
            .len()
    }

    fn parameter<T: TryFrom<i64>>(&self, name: &'static str) -> T {
        let value = self
            .get_parameter(name)
            .unwrap_or_else(|| panic!("missing parameter \"{}\" for specimen estimation", name));

        T::try_from(value).unwrap_or_else(|_| {
            panic!(
                "Failed to convert the parameter `{name}` of value `{value}` to the type `{}`",
                core::any::type_name::<T>()
            )
        })
    }
}

#[cfg(test)]
// We use a variety of weird names in these tests.
#[allow(non_camel_case_types)]
mod tests {
    use std::net::SocketAddr;

    use assert_matches::assert_matches;
    use casper_types::ProtocolVersion;
    use serde::{de::DeserializeOwned, Deserialize, Serialize};

    use crate::{components::network::handshake, protocol};

    use super::*;

    /// Version 1.0.0 network level message.
    ///
    /// Note that the message itself may go out of sync over time as `protocol::Message` changes.
    /// The test further below ensures that the handshake is accurate in the meantime.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub(crate) enum V1_0_0_Message {
        Handshake {
            /// Network we are connected to.
            network_name: String,
            /// The public address of the node connecting.
            public_address: SocketAddr,
        },
        Payload(protocol::Message),
    }

    /// A "conserved" version 1.0.0 handshake.
    ///
    /// NEVER CHANGE THIS CONSTANT TO MAKE TESTS PASS, AS IT IS BASED ON MAINNET DATA.
    const V1_0_0_HANDSHAKE: &[u8] = &[
        129, 0, 146, 178, 115, 101, 114, 105, 97, 108, 105, 122, 97, 116, 105, 111, 110, 45, 116,
        101, 115, 116, 177, 49, 50, 46, 51, 52, 46, 53, 54, 46, 55, 56, 58, 49, 50, 51, 52, 54,
    ];

    /// A "conserved" version 1.4.2 handshake.
    ///
    /// NEVER CHANGE THIS CONSTANT TO MAKE TESTS PASS, AS IT IS BASED ON TESTNET DATA.
    const V1_4_2_HANDSHAKE: &[u8] = &[
        129, 0, 148, 177, 101, 120, 97, 109, 112, 108, 101, 45, 104, 97, 110, 100, 115, 104, 97,
        107, 101, 177, 49, 50, 46, 51, 52, 46, 53, 54, 46, 55, 56, 58, 49, 50, 51, 52, 54, 165, 49,
        46, 52, 46, 50, 146, 217, 68, 48, 50, 48, 50, 56, 51, 99, 48, 68, 54, 56, 55, 57, 51, 51,
        69, 98, 50, 48, 97, 53, 52, 49, 67, 56, 53, 52, 48, 52, 55, 56, 56, 55, 55, 56, 54, 49,
        101, 100, 69, 52, 65, 70, 102, 65, 102, 48, 52, 97, 54, 56, 101, 97, 49, 57, 52, 66, 55,
        65, 52, 48, 48, 52, 54, 52, 50, 52, 101, 217, 130, 48, 50, 99, 68, 70, 65, 51, 51, 51, 99,
        49, 56, 56, 57, 51, 100, 57, 102, 51, 54, 48, 51, 53, 97, 51, 98, 55, 55, 48, 50, 51, 52,
        56, 97, 67, 102, 70, 48, 70, 68, 53, 65, 50, 65, 69, 57, 99, 66, 67, 48, 69, 52, 56, 69,
        53, 57, 100, 100, 48, 56, 53, 53, 56, 49, 97, 54, 48, 49, 53, 57, 66, 55, 102, 99, 67, 99,
        53, 52, 68, 68, 48, 70, 65, 57, 52, 52, 51, 100, 50, 69, 51, 53, 55, 51, 51, 55, 56, 68,
        54, 49, 69, 97, 49, 54, 101, 54, 53, 57, 68, 49, 54, 100, 48, 48, 48, 57, 65, 52, 48, 66,
        55, 55, 53, 48, 66, 67, 67, 69, 65, 69,
    ];

    /// A "conserved" version 1.4.3 handshake.
    ///
    /// NEVER CHANGE THIS CONSTANT TO MAKE TESTS PASS, AS IT IS BASED ON MAINNET DATA.
    const V1_4_3_HANDSHAKE: &[u8] = &[
        129, 0, 148, 177, 101, 120, 97, 109, 112, 108, 101, 45, 104, 97, 110, 100, 115, 104, 97,
        107, 101, 177, 49, 50, 46, 51, 52, 46, 53, 54, 46, 55, 56, 58, 49, 50, 51, 52, 54, 165, 49,
        46, 52, 46, 51, 146, 217, 68, 48, 50, 48, 51, 51, 49, 101, 102, 98, 102, 55, 99, 99, 51,
        51, 56, 49, 53, 49, 53, 97, 55, 50, 50, 57, 102, 57, 99, 51, 101, 55, 57, 55, 48, 51, 48,
        50, 50, 56, 99, 97, 97, 49, 56, 57, 102, 98, 50, 97, 49, 48, 50, 56, 97, 100, 101, 48, 52,
        101, 50, 57, 55, 48, 102, 55, 52, 99, 53, 217, 130, 48, 50, 55, 54, 54, 52, 56, 54, 54, 55,
        57, 52, 98, 97, 99, 99, 101, 52, 52, 49, 51, 57, 50, 102, 52, 51, 50, 100, 98, 97, 50, 100,
        101, 54, 55, 100, 97, 51, 98, 97, 55, 56, 53, 101, 53, 57, 99, 57, 52, 56, 48, 102, 49, 50,
        54, 55, 57, 52, 101, 100, 55, 56, 98, 56, 101, 53, 50, 57, 57, 57, 55, 54, 49, 99, 48, 56,
        49, 53, 56, 50, 56, 53, 53, 56, 48, 98, 52, 97, 54, 55, 98, 55, 101, 51, 52, 51, 99, 50,
        50, 56, 49, 51, 51, 99, 52, 49, 100, 52, 50, 53, 48, 98, 102, 55, 57, 100, 55, 56, 54, 100,
        55, 99, 49, 57, 57, 99, 97, 57, 55, 55,
    ];

    // Note: MessagePack messages can be visualized using the message pack visualizer at
    // https://sugendran.github.io/msgpack-visualizer/. Rust arrays can be copy&pasted and converted
    // to base64 using the following one-liner: `import base64; base64.b64encode(bytes([129, 0,
    // ...]))`

    // It is very important to note that different versions of the message pack codec crate set the
    // human-readable flag in a different manner. Thus the V1.0.0 handshake can be serialized in two
    // different ways, with "human readable" enabled and without.
    //
    // Our V1.0.0 protocol uses the "human readable" enabled version, they key difference being that
    // the `SocketAddr` is encoded as a string instead of a two-item array.

    /// A pseudo-1.0.0 handshake, where the serde human readable flag has been changed due to an
    /// `rmp` version mismatch.
    const BROKEN_V1_0_0_HANDSHAKE: &[u8] = &[
        129, 0, 146, 178, 115, 101, 114, 105, 97, 108, 105, 122, 97, 116, 105, 111, 110, 45, 116,
        101, 115, 116, 129, 0, 146, 148, 12, 34, 56, 78, 205, 48, 58,
    ];

    /// Serialize a message using the standard serialization method for handshakes.
    fn serialize_message<M: Serialize>(msg: &M) -> Vec<u8> {
        handshake::serialize(msg).expect("handshake serialization failed")
    }

    /// Deserialize a message using the standard deserialization method for handshakes.
    fn deserialize_message<M: DeserializeOwned>(serialized: &[u8]) -> M {
        handshake::deserialize(serialized).expect("message deserialization failed")
    }

    /// Given a message `from` of type `F`, serializes it, then deserializes it as `T`.
    fn roundtrip_message<F, T>(from: &F) -> T
    where
        F: Serialize,
        T: DeserializeOwned,
    {
        let serialized = serialize_message(from);
        deserialize_message(&serialized)
    }

    // This test ensure that the serialization of the `V_1_0_0_Message` has not changed and that the
    // serialization/deserialization methods for message in this test are likely accurate.
    #[test]
    fn v1_0_0_handshake_is_as_expected() {
        let handshake = V1_0_0_Message::Handshake {
            network_name: "serialization-test".to_owned(),
            public_address: ([12, 34, 56, 78], 12346).into(),
        };

        let serialized = serialize_message::<V1_0_0_Message>(&handshake);

        assert_eq!(&serialized, V1_0_0_HANDSHAKE);
        assert_ne!(&serialized, BROKEN_V1_0_0_HANDSHAKE);

        let deserialized: V1_0_0_Message = deserialize_message(&serialized);

        match deserialized {
            V1_0_0_Message::Handshake {
                network_name,
                public_address,
            } => {
                assert_eq!(network_name, "serialization-test");
                assert_eq!(public_address, ([12, 34, 56, 78], 12346).into());
            }
            other => {
                panic!("did not expect {:?} as the deserialized product", other);
            }
        }
    }

    #[test]
    fn v1_0_0_can_decode_current_handshake() {
        let mut rng = crate::new_rng();
        let modern_handshake = Message::<protocol::Message>::Handshake {
            network_name: "example-handshake".to_string(),
            public_addr: ([12, 34, 56, 78], 12346).into(),
            protocol_version: ProtocolVersion::from_parts(5, 6, 7),
            consensus_certificate: Some(ConsensusCertificate::random(&mut rng)),
            chainspec_hash: Some(Digest::hash("example-chainspec")),
        };

        let legacy_handshake: V1_0_0_Message = roundtrip_message(&modern_handshake);

        match legacy_handshake {
            V1_0_0_Message::Handshake {
                network_name,
                public_address,
            } => {
                assert_eq!(network_name, "example-handshake");
                assert_eq!(public_address, ([12, 34, 56, 78], 12346).into());
            }
            V1_0_0_Message::Payload(_) => {
                panic!("did not expect legacy handshake to deserialize to payload")
            }
        }
    }

    #[test]
    fn current_handshake_decodes_from_v1_0_0() {
        let legacy_handshake = V1_0_0_Message::Handshake {
            network_name: "example-handshake".to_string(),
            public_address: ([12, 34, 56, 78], 12346).into(),
        };

        let modern_handshake: Message<protocol::Message> = roundtrip_message(&legacy_handshake);

        if let Message::Handshake {
            network_name,
            public_addr,
            protocol_version,
            consensus_certificate,
            chainspec_hash,
        } = modern_handshake
        {
            assert_eq!(network_name, "example-handshake");
            assert_eq!(public_addr, ([12, 34, 56, 78], 12346).into());
            assert_eq!(protocol_version, ProtocolVersion::V1_0_0);
            assert!(consensus_certificate.is_none());
            assert!(chainspec_hash.is_none())
        } else {
            panic!("did not expect modern handshake to deserialize to anything but")
        }
    }

    #[test]
    fn current_handshake_decodes_from_historic_v1_0_0() {
        let modern_handshake: Message<protocol::Message> = deserialize_message(V1_0_0_HANDSHAKE);

        if let Message::Handshake {
            network_name,
            public_addr,
            protocol_version,
            consensus_certificate,
            chainspec_hash,
        } = modern_handshake
        {
            assert_eq!(network_name, "serialization-test");
            assert_eq!(public_addr, ([12, 34, 56, 78], 12346).into());
            assert_eq!(protocol_version, ProtocolVersion::V1_0_0);
            assert!(consensus_certificate.is_none());
            assert!(chainspec_hash.is_none())
        } else {
            panic!("did not expect modern handshake to deserialize to anything but")
        }
    }

    #[test]
    fn current_handshake_decodes_from_historic_v1_4_2() {
        let modern_handshake: Message<protocol::Message> = deserialize_message(V1_4_2_HANDSHAKE);

        if let Message::Handshake {
            network_name,
            public_addr,
            protocol_version,
            consensus_certificate,
            chainspec_hash,
        } = modern_handshake
        {
            assert_eq!(network_name, "example-handshake");
            assert_eq!(public_addr, ([12, 34, 56, 78], 12346).into());
            assert_eq!(protocol_version, ProtocolVersion::from_parts(1, 4, 2));
            let ConsensusCertificate {
                public_key,
                signature,
            } = consensus_certificate.unwrap();

            assert_eq!(
                public_key,
                PublicKey::from_hex(
                    "020283c0d687933eb20a541c8540478877861ede4affaf04a68ea194b7a40046424e"
                )
                .unwrap()
            );
            assert_eq!(
                signature,
                Signature::from_hex(
                    "02cdfa333c18893d9f36035a3b7702348acff0fd5a2ae9cbc0e48e59dd085581a6015\
                        9b7fccc54dd0fa9443d2e3573378d61ea16e659d16d0009a40b7750bcceae"
                )
                .unwrap()
            );
            assert!(chainspec_hash.is_none())
        } else {
            panic!("did not expect modern handshake to deserialize to anything but")
        }
    }

    #[test]
    fn current_handshake_decodes_from_historic_v1_4_3() {
        let modern_handshake: Message<protocol::Message> = deserialize_message(V1_4_3_HANDSHAKE);

        if let Message::Handshake {
            network_name,
            public_addr,
            protocol_version,
            consensus_certificate,
            chainspec_hash,
        } = modern_handshake
        {
            assert_eq!(network_name, "example-handshake");
            assert_eq!(public_addr, ([12, 34, 56, 78], 12346).into());
            assert_eq!(protocol_version, ProtocolVersion::from_parts(1, 4, 3));
            let ConsensusCertificate {
                public_key,
                signature,
            } = consensus_certificate.unwrap();

            assert_eq!(
                public_key,
                PublicKey::from_hex(
                    "020331efbf7cc3381515a7229f9c3e797030228caa189fb2a1028ade04e2970f74c5"
                )
                .unwrap()
            );
            assert_eq!(
                signature,
                Signature::from_hex(
                    "027664866794bacce441392f432dba2de67da3ba785e59c9480f126794ed78b8e5299\
                        9761c08158285580b4a67b7e343c228133c41d4250bf79d786d7c199ca977"
                )
                .unwrap()
            );
            assert!(chainspec_hash.is_none())
        } else {
            panic!("did not expect modern handshake to deserialize to anything but")
        }
    }

    fn roundtrip_certificate(use_human_readable: bool) {
        let mut rng = crate::new_rng();
        let certificate = ConsensusCertificate::random(&mut rng);

        let deserialized = if use_human_readable {
            let serialized = serde_json::to_string(&certificate).unwrap();
            serde_json::from_str(&serialized).unwrap()
        } else {
            let serialized = bincode::serialize(&certificate).unwrap();
            bincode::deserialize(&serialized).unwrap()
        };
        assert_eq!(certificate, deserialized);
    }

    #[test]
    fn serde_json_roundtrip_certificate() {
        roundtrip_certificate(true)
    }

    #[test]
    fn bincode_roundtrip_certificate() {
        roundtrip_certificate(false)
    }

    #[test]
    fn channels_enum_does_not_have_holes() {
        for idx in 0..Channel::COUNT {
            let result = Channel::from_repr(idx as u8);
            result.expect("must not have holes in channel enum");
        }
    }

    #[test]
    fn assert_the_largest_specimen_type_and_size_for_channel_network() {
        let (chainspec, _) = crate::utils::Loadable::from_resources("production");
        let _specimen = generate_largest_message(&chainspec, Channel::Network);
        //Fill the test when the channel is used.
    }

    #[test]
    fn assert_the_largest_specimen_type_and_size_for_channel_sync_data_requests() {
        let (chainspec, _) = crate::utils::Loadable::from_resources("production");
        let specimen = generate_largest_message(&chainspec, Channel::SyncDataRequests);

        assert_matches!(
            specimen,
            Message::Payload(protocol::Message::GetRequest { .. }),
            "the type of the largest possible network SyncDataRequests based on the production chainspec has changed"
        );

        let serialized = serialize_network_message(&specimen).expect("serialization failed");

        assert_eq!(
            serialized.len(),
            52,
            "the size of the largest possible SyncDataRequests message based on the production chainspec has changed"
        );
    }

    #[test]
    fn assert_the_largest_specimen_type_and_size_for_channel_sync_data_responses() {
        let (chainspec, _) = crate::utils::Loadable::from_resources("production");
        let specimen = generate_largest_message(&chainspec, Channel::SyncDataResponses);

        assert_matches!(
            specimen,
            Message::Payload(protocol::Message::GetResponse { .. }),
            "the type of the largest possible SyncDataResponses message based on the production chainspec has changed"
        );

        let serialized = serialize_network_message(&specimen).expect("serialization failed");

        assert_eq!(
            serialized.len(),
            8_388_736,
            "the size of the largest possible SyncDataResponses message based on the production chainspec has changed"
        );
    }

    #[test]
    fn assert_the_largest_specimen_type_and_size_for_channel_data_request() {
        let (chainspec, _) = crate::utils::Loadable::from_resources("production");
        let specimen = generate_largest_message(&chainspec, Channel::DataRequests);

        assert_matches!(
            specimen,
            Message::Payload(protocol::Message::GetRequest { .. }),
            "the type of the largest possible DataRequests message based on the production chainspec has changed"
        );

        let serialized = serialize_network_message(&specimen).expect("serialization failed");

        assert_eq!(
            serialized.len(),
            97,
            "the size of the largest possible DataRequests message based on the production chainspec has changed"
        );
    }

    #[test]
    fn assert_the_largest_specimen_type_and_size_for_channel_data_responses() {
        let (chainspec, _) = crate::utils::Loadable::from_resources("production");
        let specimen = generate_largest_message(&chainspec, Channel::DataResponses);

        assert_matches!(
            specimen,
            Message::Payload(protocol::Message::GetResponse { .. }),
            "the type of the largest possible DataResponses message based on the production chainspec has changed"
        );

        let serialized = serialize_network_message(&specimen).expect("serialization failed");

        assert_eq!(
            serialized.len(),
            1_048_977,
            "the size of the largest possible DataResponses message based on the production chainspec has changed"
        );
    }

    #[test]
    fn assert_the_largest_specimen_type_and_size_for_channel_consensus() {
        let (chainspec, _) = crate::utils::Loadable::from_resources("production");
        let specimen = generate_largest_message(&chainspec, Channel::Consensus);

        assert_matches!(
            specimen,
            Message::Payload(protocol::Message::Consensus { .. }),
            "the type of the largest possible Consensus message based on the production chainspec has changed"
        );

        let serialized = serialize_network_message(&specimen).expect("serialization failed");

        assert_eq!(
            serialized.len(),
            907_737,
            "the size of the largest possible Consensus message based on the production chainspec has changed"
        );
    }

    #[test]
    fn assert_the_largest_specimen_type_and_size_for_channel_bulk_gossip() {
        let (chainspec, _) = crate::utils::Loadable::from_resources("production");
        let specimen = generate_largest_message(&chainspec, Channel::BulkGossip);

        assert_matches!(
            specimen,
            Message::Payload(protocol::Message::DeployGossiper { .. }),
            "the type of the largest possible BulkGossip message based on the production chainspec has changed"
        );

        let serialized = serialize_network_message(&specimen).expect("serialization failed");

        assert_eq!(
            serialized.len(),
            1_048_883,
            "the size of the largest possible BulkGossip message based on the production chainspec has changed"
        );
    }
}
