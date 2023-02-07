//! Specimen support.
//!
//! Structs implementing the specimen trait allow for specific sample instances being created, such
//! as the biggest possible.

use core::convert::TryInto;
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    iter::FromIterator,
    net::{Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::Arc,
};

use casper_execution_engine::core::engine_state::{
    executable_deploy_item::ExecutableDeployItemDiscriminants, ExecutableDeployItem,
};
use casper_hashing::{ChunkWithProof, Digest};
use casper_types::{
    bytesrepr::Bytes,
    crypto::{sign, PublicKey, PublicKeyDiscriminants, Signature},
    AsymmetricType, ContractHash, ContractPackageHash, EraId, ProtocolVersion, RuntimeArgs,
    SecretKey, SemVer, SignatureDiscriminants, TimeDiff, Timestamp, KEY_HASH_LENGTH, U512,
};
use either::Either;
use serde::Serialize;
use strum::IntoEnumIterator;

use crate::{
    components::{
        consensus::{max_rounds_per_era, utils::ValidatorMap, EraReport},
        fetcher::Tag,
    },
    protocol::Message,
    testing::TestRng,
    types::{
        Approval, ApprovalsHash, ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHash,
        BlockHeader, BlockPayload, Deploy, DeployHashWithApprovals, DeployId, FinalitySignature,
        FinalitySignatureId, FinalizedBlock, LegacyDeploy, SyncLeap, TrieOrChunk,
    },
};

/// The largest valid unicode codepoint that can be encoded to UTF-8.
pub(crate) const HIGHEST_UNICODE_CODEPOINT: char = '\u{10FFFF}';

/// Given a specific type instance, estimates its serialized size.
pub(crate) trait SizeEstimator {
    /// Estimate the serialized size of a value.
    fn estimate<T: Serialize>(&self, val: &T) -> usize;

    /// Retrieves a parameter.
    ///
    /// Parameters indicate potential specimens which values to expect, e.g. a maximum number of
    /// items configured for a specific collection. If `None` is returned a default should be used
    /// by the caller, or a panic produced.
    fn get_parameter(&self, name: &'static str) -> Option<i64>;

    /// Requires a parameter.
    ///
    /// Like `get_parameter`, but does not accept `None` as an answer, and it
    /// returns the parameter converted to the asked type.
    ///
    /// ## Panics
    ///
    /// - If the named parameter is not set, panics.
    /// - If `T` cannot be converted from `i64`.
    fn require_parameter<T: TryFrom<i64>>(&self, name: &'static str) -> T {
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

/// Supports returning a maximum size specimen.
///
/// "Maximum size" refers to the instance that uses the highest amount of memory and is also most
/// likely to have the largest representation when serialized.
pub(crate) trait LargestSpecimen {
    /// Returns the largest possible specimen for this type.
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self;
}

/// Supports generating a unique sequence of specimen that are as large as possible.
pub(crate) trait LargeUniqueSequence<E>
where
    Self: Sized + Ord,
    E: SizeEstimator,
{
    /// Create a new sequence of the largest possible unique specimens.
    ///
    /// Note that multiple calls to this function will return overlapping sequences.
    // Note: This functions returns a materialized sequence instead of a generator to avoid
    //       complications with borrowing `E`.
    fn large_unique_sequence(estimator: &E, count: usize) -> BTreeSet<Self>;
}

/// Produces the largest variant of a specific `enum` using an estimator and a generation function.
pub(crate) fn largest_variant<T, D, E, F>(estimator: &E, generator: F) -> T
where
    T: Serialize,
    D: IntoEnumIterator,
    E: SizeEstimator,
    F: FnMut(D) -> T,
{
    D::iter()
        .map(generator)
        .max_by_key(|candidate| estimator.estimate(candidate))
        .expect("should have at least one candidate")
}

/// Generates a vec of a given size filled with the largest specimen.
pub(crate) fn vec_of_largest_specimen<T: LargestSpecimen, E: SizeEstimator>(
    estimator: &E,
    count: usize,
) -> Vec<T> {
    let mut vec = Vec::new();
    for _ in 0..count {
        vec.push(LargestSpecimen::largest_specimen(estimator));
    }
    vec
}

/// Generates a vec of the largest specimen, with a size from a property.
pub(crate) fn vec_prop_specimen<T: LargestSpecimen, E: SizeEstimator>(
    estimator: &E,
    parameter_name: &'static str,
) -> Vec<T> {
    let mut count = estimator.require_parameter(parameter_name);
    if count < 0 {
        count = 0;
    }

    vec_of_largest_specimen(estimator, count as usize)
}

/// Generates a `BTreeMap` with the size taken from a property.
///
/// Keys are generated uniquely using `LargeUniqueSequence`, while values will be largest specimen.
pub(crate) fn btree_map_distinct_from_prop<K, V, E>(
    estimator: &E,
    parameter_name: &'static str,
) -> BTreeMap<K, V>
where
    V: LargestSpecimen,
    K: Ord + LargeUniqueSequence<E> + Sized,
    E: SizeEstimator,
{
    let mut count = estimator.require_parameter(parameter_name);
    if count < 0 {
        count = 0;
    }

    K::large_unique_sequence(estimator, count as usize)
        .into_iter()
        .map(|key| (key, LargestSpecimen::largest_specimen(estimator)))
        .collect()
}

/// Generates a `BTreeSet` with the size taken from a property.
///
/// Value are generated uniquely using `LargeUniqueSequence`.
pub(crate) fn btree_set_distinct_from_prop<T, E>(
    estimator: &E,
    parameter_name: &'static str,
) -> BTreeSet<T>
where
    T: Ord + LargeUniqueSequence<E> + Sized,
    E: SizeEstimator,
{
    let mut count = estimator.require_parameter(parameter_name);
    if count < 0 {
        count = 0;
    }

    T::large_unique_sequence(estimator, count as usize)
}

/// Generates a `BTreeSet` with a given amount of items.
///
/// Value are generated uniquely using `LargeUniqueSequence`.
pub(crate) fn btree_set_distinct<T, E>(estimator: &E, count: usize) -> BTreeSet<T>
where
    T: Ord + LargeUniqueSequence<E> + Sized,
    E: SizeEstimator,
{
    T::large_unique_sequence(estimator, count)
}

impl LargestSpecimen for SocketAddr {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        SocketAddr::V6(SocketAddrV6::largest_specimen(estimator))
    }
}

impl LargestSpecimen for SocketAddrV6 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        SocketAddrV6::new(
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

impl LargestSpecimen for Ipv6Addr {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        // Leading zeros get shorted, ensure there are none in the address.
        Ipv6Addr::new(
            0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff,
        )
    }
}

impl LargestSpecimen for bool {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        true
    }
}

impl LargestSpecimen for u8 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        u8::MAX
    }
}

impl LargestSpecimen for u16 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        u16::MAX
    }
}

impl LargestSpecimen for u32 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        u32::MAX
    }
}

impl LargestSpecimen for u64 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        u64::MAX
    }
}

impl LargestSpecimen for u128 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        u128::MAX
    }
}

impl<T: LargestSpecimen + Copy, const N: usize> LargestSpecimen for [T; N] {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        [LargestSpecimen::largest_specimen(estimator); N]
    }
}

impl<T> LargestSpecimen for Option<T>
where
    T: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        Some(LargestSpecimen::largest_specimen(estimator))
    }
}

impl<T> LargestSpecimen for Box<T>
where
    T: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        Box::new(LargestSpecimen::largest_specimen(estimator))
    }
}

impl<T> LargestSpecimen for Arc<T>
where
    T: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        Arc::new(LargestSpecimen::largest_specimen(estimator))
    }
}

impl<T1, T2> LargestSpecimen for (T1, T2)
where
    T1: LargestSpecimen,
    T2: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        (
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

impl<T1, T2, T3> LargestSpecimen for (T1, T2, T3)
where
    T1: LargestSpecimen,
    T2: LargestSpecimen,
    T3: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        (
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

// various third party crates

impl<L, R> LargestSpecimen for Either<L, R>
where
    L: LargestSpecimen + Serialize,
    R: LargestSpecimen + Serialize,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        let l = L::largest_specimen(estimator);
        let r = R::largest_specimen(estimator);

        if estimator.estimate(&l) >= estimator.estimate(&r) {
            Either::Left(l)
        } else {
            Either::Right(r)
        }
    }
}

// impls for `casper_types`, which is technically a foreign crate -- so we put them here.
impl LargestSpecimen for ProtocolVersion {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        ProtocolVersion::new(LargestSpecimen::largest_specimen(estimator))
    }
}

impl LargestSpecimen for SemVer {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        SemVer {
            major: LargestSpecimen::largest_specimen(estimator),
            minor: LargestSpecimen::largest_specimen(estimator),
            patch: LargestSpecimen::largest_specimen(estimator),
        }
    }
}

impl LargestSpecimen for PublicKey {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        largest_random_public_key(estimator, &mut TestRng::new())
    }
}

fn largest_random_public_key<E: SizeEstimator>(estimator: &E, rng: &mut TestRng) -> PublicKey {
    largest_variant::<PublicKey, PublicKeyDiscriminants, _, _>(estimator, move |variant| {
        match variant {
            PublicKeyDiscriminants::System => PublicKey::system(),
            PublicKeyDiscriminants::Ed25519 => PublicKey::random_ed25519(rng),
            PublicKeyDiscriminants::Secp256k1 => PublicKey::random_secp256k1(rng),
        }
    })
}

impl<E> LargeUniqueSequence<E> for PublicKey
where
    E: SizeEstimator,
{
    fn large_unique_sequence(estimator: &E, count: usize) -> BTreeSet<Self> {
        let mut rng = TestRng::new();

        (0..count)
            .map(move |_| largest_random_public_key(estimator, &mut rng))
            .collect()
    }
}

impl<E> LargeUniqueSequence<E> for Digest
where
    E: SizeEstimator,
{
    fn large_unique_sequence(_estimator: &E, count: usize) -> BTreeSet<Self> {
        (0..count).map(|n| Digest::hash(n.to_ne_bytes())).collect()
    }
}

impl LargestSpecimen for Signature {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        let ed25519_sec = &SecretKey::generate_ed25519().expect("a correct secret");
        let secp256k1_sec = &SecretKey::generate_secp256k1().expect("a correct secret");

        largest_variant::<Self, SignatureDiscriminants, _, _>(estimator, |variant| match variant {
            SignatureDiscriminants::System => Signature::system(),
            SignatureDiscriminants::Ed25519 => sign([0_u8], ed25519_sec, &ed25519_sec.into()),
            SignatureDiscriminants::Secp256k1 => sign([0_u8], secp256k1_sec, &secp256k1_sec.into()),
        })
    }
}

impl LargestSpecimen for EraId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        EraId::new(LargestSpecimen::largest_specimen(estimator))
    }
}

impl LargestSpecimen for Timestamp {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        Timestamp::MAX
    }
}

impl LargestSpecimen for TimeDiff {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        TimeDiff::from_millis(LargestSpecimen::largest_specimen(estimator))
    }
}

impl LargestSpecimen for Block {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        Block::new(
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            Some(btree_map_distinct_from_prop(estimator, "validator_count")),
            LargestSpecimen::largest_specimen(estimator),
        )
        .expect("did not expect largest speciment creation of block to fail")
    }
}

impl LargestSpecimen for FinalizedBlock {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        FinalizedBlock::new(
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

impl LargestSpecimen for FinalitySignature {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        FinalitySignature::new(
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

impl LargestSpecimen for FinalitySignatureId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        FinalitySignatureId {
            block_hash: LargestSpecimen::largest_specimen(estimator),
            era_id: LargestSpecimen::largest_specimen(estimator),
            public_key: LargestSpecimen::largest_specimen(estimator),
        }
    }
}

impl LargestSpecimen for EraReport<PublicKey> {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        EraReport {
            equivocators: vec_prop_specimen(estimator, "validator_count"),
            rewards: btree_map_distinct_from_prop(estimator, "validator_count"),
            inactive_validators: vec_prop_specimen(estimator, "validator_count"),
        }
    }
}

impl LargestSpecimen for BlockHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        BlockHash::new(LargestSpecimen::largest_specimen(estimator))
    }
}

// impls for `casper_hashing`, which is technically a foreign crate -- so we put them here.
impl LargestSpecimen for Digest {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        // Hashes are fixed size by definition, so any value will do.
        Digest::hash("")
    }
}

impl LargestSpecimen for BlockPayload {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        BlockPayload::new(
            vec_prop_specimen(estimator, "max_deploys_per_block"),
            vec_prop_specimen(estimator, "max_transfers_per_block"),
            vec_prop_specimen(estimator, "max_accusations_per_block"),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

impl LargestSpecimen for DeployHashWithApprovals {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        // Note: This is an upper bound, the actual value is lower. We are keeping the order of
        //       magnitude intact though.
        let max_items = estimator.require_parameter::<usize>("max_deploys_per_block")
            + estimator.require_parameter::<usize>("max_transfers_per_block");
        DeployHashWithApprovals::new(
            LargestSpecimen::largest_specimen(estimator),
            btree_set_distinct(estimator, max_items),
        )
    }
}

impl LargestSpecimen for Deploy {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        Deploy::new(
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            vec![/* Legacy field, always empty */],
            largest_chain_name(estimator),
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
            &LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

impl LargestSpecimen for DeployId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        DeployId::new(
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

impl LargestSpecimen for Approval {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        Approval::from_parts(
            LargestSpecimen::largest_specimen(estimator),
            LargestSpecimen::largest_specimen(estimator),
        )
    }
}

impl<E> LargeUniqueSequence<E> for Approval
where
    Self: Sized + Ord,
    E: SizeEstimator,
{
    fn large_unique_sequence(estimator: &E, count: usize) -> BTreeSet<Self> {
        PublicKey::large_unique_sequence(estimator, count)
            .into_iter()
            .map(|pk| Approval::from_parts(pk, LargestSpecimen::largest_specimen(estimator)))
            .collect()
    }
}

impl LargestSpecimen for ApprovalsHash {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        ApprovalsHash::compute(&Default::default()).expect("empty approvals hash should compute")
    }
}

// EE impls
impl LargestSpecimen for ExecutableDeployItem {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        largest_variant::<Self, ExecutableDeployItemDiscriminants, _, _>(estimator, |variant| {
            match variant {
                ExecutableDeployItemDiscriminants::ModuleBytes => {
                    ExecutableDeployItem::ModuleBytes {
                        module_bytes: Bytes::from(vec_prop_specimen(estimator, "module_bytes")),
                        args: LargestSpecimen::largest_specimen(estimator),
                    }
                }
                ExecutableDeployItemDiscriminants::StoredContractByHash => {
                    ExecutableDeployItem::StoredContractByHash {
                        hash: LargestSpecimen::largest_specimen(estimator),
                        entry_point: largest_contract_entry_point(estimator),
                        args: LargestSpecimen::largest_specimen(estimator),
                    }
                }
                ExecutableDeployItemDiscriminants::StoredContractByName => {
                    ExecutableDeployItem::StoredContractByName {
                        name: largest_contract_name(estimator),
                        entry_point: largest_contract_entry_point(estimator),
                        args: LargestSpecimen::largest_specimen(estimator),
                    }
                }
                ExecutableDeployItemDiscriminants::StoredVersionedContractByHash => {
                    ExecutableDeployItem::StoredVersionedContractByHash {
                        hash: LargestSpecimen::largest_specimen(estimator),
                        version: LargestSpecimen::largest_specimen(estimator),
                        entry_point: largest_contract_entry_point(estimator),
                        args: LargestSpecimen::largest_specimen(estimator),
                    }
                }
                ExecutableDeployItemDiscriminants::StoredVersionedContractByName => {
                    ExecutableDeployItem::StoredVersionedContractByName {
                        name: largest_contract_name(estimator),
                        version: LargestSpecimen::largest_specimen(estimator),
                        entry_point: largest_contract_entry_point(estimator),
                        args: LargestSpecimen::largest_specimen(estimator),
                    }
                }
                ExecutableDeployItemDiscriminants::Transfer => ExecutableDeployItem::Transfer {
                    args: LargestSpecimen::largest_specimen(estimator),
                },
            }
        })
    }
}

impl LargestSpecimen for U512 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        U512::max_value()
    }
}

impl LargestSpecimen for ContractHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        ContractHash::new([LargestSpecimen::largest_specimen(estimator); KEY_HASH_LENGTH])
    }
}

impl LargestSpecimen for ContractPackageHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        ContractPackageHash::new([LargestSpecimen::largest_specimen(estimator); KEY_HASH_LENGTH])
    }
}

impl LargestSpecimen for RuntimeArgs {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        Default::default()
    }
}

impl LargestSpecimen for ChunkWithProof {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        ChunkWithProof::new(&[0xFF; 8 * 1024 * 1024], 0).expect("the chunk to be correctly created")
    }
}

impl LargestSpecimen for SecretKey {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        SecretKey::ed25519_from_bytes([u8::MAX; 32]).expect("valid secret key bytes")
    }
}

impl<T: LargestSpecimen> LargestSpecimen for ValidatorMap<T> {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        let max_validators = estimator.require_parameter("validator_count");

        ValidatorMap::from_iter(
            std::iter::repeat_with(|| LargestSpecimen::largest_specimen(estimator))
                .take(max_validators),
        )
    }
}

/// Returns the largest `Message::GetRequest`.
pub(crate) fn largest_get_request<E: SizeEstimator>(estimator: &E) -> Message {
    largest_variant::<Message, Tag, _, _>(estimator, |variant| {
        match variant {
            Tag::Deploy => {
                Message::new_get_request::<Deploy>(&LargestSpecimen::largest_specimen(estimator))
            }
            Tag::LegacyDeploy => Message::new_get_request::<LegacyDeploy>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::Block => {
                Message::new_get_request::<Block>(&LargestSpecimen::largest_specimen(estimator))
            }
            Tag::BlockHeader => Message::new_get_request::<BlockHeader>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::TrieOrChunk => Message::new_get_request::<TrieOrChunk>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::FinalitySignature => Message::new_get_request::<FinalitySignature>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::SyncLeap => {
                Message::new_get_request::<SyncLeap>(&LargestSpecimen::largest_specimen(estimator))
            }
            Tag::ApprovalsHashes => Message::new_get_request::<ApprovalsHashes>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::BlockExecutionResults => Message::new_get_request::<BlockExecutionResultsOrChunk>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
        }
        .expect("did not expect new_get_request from largest deploy to fail")
    })
}

/// Returns the largest `Message::GetResponse`.
pub(crate) fn largest_get_response<E: SizeEstimator>(estimator: &E) -> Message {
    largest_variant::<Message, Tag, _, _>(estimator, |variant| {
        match variant {
            Tag::Deploy => {
                Message::new_get_response::<Deploy>(&LargestSpecimen::largest_specimen(estimator))
            }
            Tag::LegacyDeploy => Message::new_get_response::<LegacyDeploy>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::Block => {
                Message::new_get_response::<Block>(&LargestSpecimen::largest_specimen(estimator))
            }
            Tag::BlockHeader => Message::new_get_response::<BlockHeader>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::TrieOrChunk => Message::new_get_response::<TrieOrChunk>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::FinalitySignature => Message::new_get_response::<FinalitySignature>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::SyncLeap => {
                Message::new_get_response::<SyncLeap>(&LargestSpecimen::largest_specimen(estimator))
            }
            Tag::ApprovalsHashes => Message::new_get_response::<ApprovalsHashes>(
                &LargestSpecimen::largest_specimen(estimator),
            ),
            Tag::BlockExecutionResults => {
                Message::new_get_response::<BlockExecutionResultsOrChunk>(
                    &LargestSpecimen::largest_specimen(estimator),
                )
            }
        }
        .expect("did not expect new_get_response from largest deploy to fail")
    })
}

/// Returns the largest string allowed for a contract name.
fn largest_contract_name<E: SizeEstimator>(estimator: &E) -> String {
    string_max_characters(estimator.require_parameter("contract_name_limit"))
}

/// Returns the largest string allowed for a chain name.
fn largest_chain_name<E: SizeEstimator>(estimator: &E) -> String {
    string_max_characters(estimator.require_parameter("network_name_limit"))
}

/// Returns the largest string allowed for a contract entry point.
fn largest_contract_entry_point<E: SizeEstimator>(estimator: &E) -> String {
    string_max_characters(estimator.require_parameter("entry_point_limit"))
}

/// Returns a string with `len`s characters of the largest possible size.
fn string_max_characters(max_char: usize) -> String {
    std::iter::repeat(HIGHEST_UNICODE_CODEPOINT)
        .take(max_char)
        .collect()
}

/// Returns the max rounds per era with the specimen parameters.
///
/// See the [`max_rounds_per_era`] function.
pub(crate) fn estimator_max_rounds_per_era(estimator: &impl SizeEstimator) -> usize {
    let minimum_era_height = estimator.require_parameter("minimum_era_height");
    let era_duration_ms = TimeDiff::from_millis(estimator.require_parameter("era_duration_ms"));
    let minimum_round_length_ms =
        TimeDiff::from_millis(estimator.require_parameter("minimum_round_length_ms"));

    max_rounds_per_era(minimum_era_height, era_duration_ms, minimum_round_length_ms)
        .try_into()
        .expect("to be a valid `usize`")
}
