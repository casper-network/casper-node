//! Specimen support.
//!
//! Structs implementing the specimen trait allow for specific sample instances being created, such
//! as the biggest possible.

use std::{
    any::{Any, TypeId},
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::{TryFrom, TryInto},
    iter::FromIterator,
    net::{Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::Arc,
};

use either::Either;
use once_cell::sync::OnceCell;
use serde::Serialize;
use strum::{EnumIter, IntoEnumIterator};

use casper_types::{
    account::AccountHash,
    bytesrepr::Bytes,
    crypto::{sign, PublicKey, Signature},
    AccessRights, AsymmetricType, Block, BlockHash, BlockHeader, BlockHeaderV1, BlockHeaderV2,
    BlockSignatures, BlockSignaturesV2, BlockV2, ChainNameDigest, ChunkWithProof, Deploy,
    DeployApproval, DeployApprovalsHash, DeployHash, DeployId, Digest, EraEndV1, EraEndV2, EraId,
    EraReport, ExecutableDeployItem, FinalitySignature, FinalitySignatureId, FinalitySignatureV2,
    PackageHash, ProtocolVersion, RewardedSignatures, RuntimeArgs, SecretKey, SemVer,
    SignedBlockHeader, SingleBlockRewardedSignatures, TimeDiff, Timestamp, Transaction,
    TransactionApprovalsHash, TransactionHash, TransactionId, TransactionV1, TransactionV1Approval,
    TransactionV1ApprovalsHash, TransactionV1Builder, TransactionV1Hash, URef, KEY_HASH_LENGTH,
    U512,
};

use crate::{
    components::{
        consensus::{max_rounds_per_era, utils::ValidatorMap},
        fetcher::Tag,
    },
    protocol::Message,
    types::{
        ApprovalsHashes, BlockExecutionResultsOrChunk, BlockPayload, DeployHashWithApprovals,
        FinalizedBlock, InternalEraReport, LegacyDeploy, SyncLeap, TransactionHashWithApprovals,
        TrieOrChunk,
    },
};

/// The largest valid unicode codepoint that can be encoded to UTF-8.
pub(crate) const HIGHEST_UNICODE_CODEPOINT: char = '\u{10FFFF}';

/// A cache used for memoization, typically on a single estimator.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    /// A map of items that have been hashed. Indexed by type.
    items: HashMap<TypeId, Vec<Box<dyn Any>>>,
}

impl Cache {
    /// Retrieves a potentially memoized instance.
    pub(crate) fn get<T: Any>(&mut self) -> Option<&T> {
        self.get_all::<T>()
            .get(0)
            .map(|box_any| box_any.downcast_ref::<T>().expect("cache corrupted"))
    }

    /// Sets the memoized instance if not already set.
    ///
    /// Returns a reference to the memoized instance. Note that this may be an instance other than
    /// the passed in `item`, if the cache entry was not empty before/
    pub(crate) fn set<T: Any>(&mut self, item: T) -> &T {
        let items = self.get_all::<T>();
        if items.is_empty() {
            let boxed_item: Box<dyn Any> = Box::new(item);
            items.push(boxed_item);
        }
        self.get::<T>().expect("should not be empty")
    }

    /// Get or insert the vector storing item instances.
    fn get_all<T: Any>(&mut self) -> &mut Vec<Box<dyn Any>> {
        self.items.entry(TypeId::of::<T>()).or_default()
    }
}

/// Given a specific type instance, estimates its serialized size.
pub(crate) trait SizeEstimator {
    /// Estimate the serialized size of a value.
    fn estimate<T: Serialize>(&self, val: &T) -> usize;

    /// Requires a parameter.
    ///
    /// Parameters indicate potential specimens which values to expect, e.g. a maximum number of
    /// items configured for a specific collection.
    ///
    /// ## Panics
    ///
    /// - If the named parameter is not set, panics.
    /// - If `T` is of an invalid type.
    fn parameter<T: TryFrom<i64>>(&self, name: &'static str) -> T;

    /// Require a parameter, cast into a boolean.
    ///
    /// See [`parameter`] for details. Will return `false` if the stored value is `0`,
    /// otherwise `true`.
    ///
    /// This method exists because `bool` does not implement `TryFrom<i64>`.
    ///
    /// ## Panics
    ///
    /// Same as [`parameter`].
    fn parameter_bool(&self, name: &'static str) -> bool {
        self.parameter::<i64>(name) != 0
    }
}

/// Supports returning a maximum size specimen.
///
/// "Maximum size" refers to the instance that uses the highest amount of memory and is also most
/// likely to have the largest representation when serialized.
pub(crate) trait LargestSpecimen: Sized {
    /// Returns the largest possible specimen for this type.
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self;
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
    fn large_unique_sequence(estimator: &E, count: usize, cache: &mut Cache) -> BTreeSet<Self>;
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
    cache: &mut Cache,
) -> Vec<T> {
    let mut vec = Vec::new();
    for _ in 0..count {
        vec.push(LargestSpecimen::largest_specimen(estimator, cache));
    }
    vec
}

/// Generates a vec of the largest specimen, with a size from a property.
pub(crate) fn vec_prop_specimen<T: LargestSpecimen, E: SizeEstimator>(
    estimator: &E,
    parameter_name: &'static str,
    cache: &mut Cache,
) -> Vec<T> {
    let mut count = estimator.parameter(parameter_name);
    if count < 0 {
        count = 0;
    }

    vec_of_largest_specimen(estimator, count as usize, cache)
}

/// Generates a `BTreeMap` with the size taken from a property.
///
/// Keys are generated uniquely using `LargeUniqueSequence`, while values will be largest specimen.
pub(crate) fn btree_map_distinct_from_prop<K, V, E>(
    estimator: &E,
    parameter_name: &'static str,
    cache: &mut Cache,
) -> BTreeMap<K, V>
where
    V: LargestSpecimen,
    K: Ord + LargeUniqueSequence<E> + Sized,
    E: SizeEstimator,
{
    let mut count = estimator.parameter(parameter_name);
    if count < 0 {
        count = 0;
    }

    K::large_unique_sequence(estimator, count as usize, cache)
        .into_iter()
        .map(|key| (key, LargestSpecimen::largest_specimen(estimator, cache)))
        .collect()
}

/// Generates a `BTreeSet` with the size taken from a property.
///
/// Value are generated uniquely using `LargeUniqueSequence`.
pub(crate) fn btree_set_distinct_from_prop<T, E>(
    estimator: &E,
    parameter_name: &'static str,
    cache: &mut Cache,
) -> BTreeSet<T>
where
    T: Ord + LargeUniqueSequence<E> + Sized,
    E: SizeEstimator,
{
    let mut count = estimator.parameter(parameter_name);
    if count < 0 {
        count = 0;
    }

    T::large_unique_sequence(estimator, count as usize, cache)
}

/// Generates a `BTreeSet` with a given amount of items.
///
/// Value are generated uniquely using `LargeUniqueSequence`.
pub(crate) fn btree_set_distinct<T, E>(
    estimator: &E,
    count: usize,
    cache: &mut Cache,
) -> BTreeSet<T>
where
    T: Ord + LargeUniqueSequence<E> + Sized,
    E: SizeEstimator,
{
    T::large_unique_sequence(estimator, count, cache)
}

impl LargestSpecimen for SocketAddr {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        SocketAddr::V6(SocketAddrV6::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for SocketAddrV6 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        SocketAddrV6::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for Ipv6Addr {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        // Leading zeros get shorted, ensure there are none in the address.
        Ipv6Addr::new(
            0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff,
        )
    }
}

impl LargestSpecimen for bool {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        true
    }
}

impl LargestSpecimen for u8 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        u8::MAX
    }
}

impl LargestSpecimen for u16 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        u16::MAX
    }
}

impl LargestSpecimen for u32 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        u32::MAX
    }
}

impl LargestSpecimen for u64 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        u64::MAX
    }
}

impl LargestSpecimen for u128 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        u128::MAX
    }
}

impl<T: LargestSpecimen + Copy, const N: usize> LargestSpecimen for [T; N] {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        [LargestSpecimen::largest_specimen(estimator, cache); N]
    }
}

impl<T> LargestSpecimen for Option<T>
where
    T: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        Some(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl<T> LargestSpecimen for Box<T>
where
    T: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        Box::new(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl<T> LargestSpecimen for Arc<T>
where
    T: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        Arc::new(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl<T1, T2> LargestSpecimen for (T1, T2)
where
    T1: LargestSpecimen,
    T2: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        (
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl<T1, T2, T3> LargestSpecimen for (T1, T2, T3)
where
    T1: LargestSpecimen,
    T2: LargestSpecimen,
    T3: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        (
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

// Various third party crates.

impl<L, R> LargestSpecimen for Either<L, R>
where
    L: LargestSpecimen + Serialize,
    R: LargestSpecimen + Serialize,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let l = L::largest_specimen(estimator, cache);
        let r = R::largest_specimen(estimator, cache);

        if estimator.estimate(&l) >= estimator.estimate(&r) {
            Either::Left(l)
        } else {
            Either::Right(r)
        }
    }
}

// impls for `casper_types`, which is technically a foreign crate -- so we put them here.
impl LargestSpecimen for ProtocolVersion {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        ProtocolVersion::new(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for URef {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        URef::new(
            [LargestSpecimen::largest_specimen(estimator, cache); 32],
            AccessRights::READ_ADD_WRITE,
        )
    }
}

impl LargestSpecimen for AccountHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        AccountHash::new([LargestSpecimen::largest_specimen(estimator, cache); 32])
    }
}

impl LargestSpecimen for SemVer {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        SemVer {
            major: LargestSpecimen::largest_specimen(estimator, cache),
            minor: LargestSpecimen::largest_specimen(estimator, cache),
            patch: LargestSpecimen::largest_specimen(estimator, cache),
        }
    }
}

impl LargestSpecimen for PublicKey {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        PublicKey::large_unique_sequence(estimator, 1, cache)
            .into_iter()
            .next()
            .unwrap()
    }
}

// Dummy implementation to replace the buggy real one below:
impl<E> LargeUniqueSequence<E> for PublicKey
where
    E: SizeEstimator,
{
    fn large_unique_sequence(estimator: &E, count: usize, cache: &mut Cache) -> BTreeSet<Self> {
        let data_vec = cache.get_all::<Self>();

        /// Generates a secret key from a fixed, numbered seed.
        fn generate_key<E: SizeEstimator>(estimator: &E, seed: usize) -> PublicKey {
            // Like `Signature`, we do not wish to pollute the types crate here.
            #[derive(Copy, Clone, Debug, EnumIter)]
            enum PublicKeyDiscriminants {
                System,
                Ed25519,
                Secp256k1,
            }
            largest_variant::<PublicKey, PublicKeyDiscriminants, _, _>(estimator, |variant| {
                // We take advantage of two things here:
                //
                // 1. The required seed bytes for Ed25519 and Secp256k1 are both the same length of
                //    32 bytes.
                // 2. While Secp256k1 does not allow the most trivial seed bytes of 0x00..0001, a
                //    a hash function output seems to satisfy it, and our current hashing scheme
                //    also output 32 bytes.
                let seed_bytes = Digest::hash(seed.to_be_bytes()).value();

                match variant {
                    PublicKeyDiscriminants::System => PublicKey::system(),
                    PublicKeyDiscriminants::Ed25519 => {
                        let ed25519_sec = SecretKey::ed25519_from_bytes(seed_bytes)
                            .expect("unable to create ed25519 key from seed bytes");
                        PublicKey::from(&ed25519_sec)
                    }
                    PublicKeyDiscriminants::Secp256k1 => {
                        let secp256k1_sec = SecretKey::secp256k1_from_bytes(seed_bytes)
                            .expect("unable to create secp256k1 key from seed bytes");
                        PublicKey::from(&secp256k1_sec)
                    }
                }
            })
        }

        while data_vec.len() < count {
            let seed = data_vec.len();
            let key = generate_key(estimator, seed);
            data_vec.push(Box::new(key));
        }

        debug_assert!(data_vec.len() >= count);
        let output_set: BTreeSet<Self> = data_vec[..count]
            .iter()
            .map(|item| item.downcast_ref::<Self>().expect("cache corrupted"))
            .cloned()
            .collect();
        debug_assert_eq!(output_set.len(), count);

        output_set
    }
}

impl<E> LargeUniqueSequence<E> for Digest
where
    E: SizeEstimator,
{
    fn large_unique_sequence(_estimator: &E, count: usize, _cache: &mut Cache) -> BTreeSet<Self> {
        (0..count).map(|n| Digest::hash(n.to_ne_bytes())).collect()
    }
}

impl LargestSpecimen for Signature {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        if let Some(item) = cache.get::<Self>() {
            return *item;
        }

        // Note: We do not use strum generated discriminator enums for the signature, as we do not
        //       want to make `strum` a direct dependency of `casper-types`, to keep its size down.
        #[derive(Debug, Copy, Clone, EnumIter)]
        enum SignatureDiscriminants {
            System,
            Ed25519,
            Secp256k1,
        }

        *cache.set(largest_variant::<Self, SignatureDiscriminants, _, _>(
            estimator,
            |variant| match variant {
                SignatureDiscriminants::System => Signature::system(),
                SignatureDiscriminants::Ed25519 => {
                    let ed25519_sec = &SecretKey::generate_ed25519().expect("a correct secret");

                    sign([0_u8], ed25519_sec, &ed25519_sec.into())
                }
                SignatureDiscriminants::Secp256k1 => {
                    let secp256k1_sec = &SecretKey::generate_secp256k1().expect("a correct secret");

                    sign([0_u8], secp256k1_sec, &secp256k1_sec.into())
                }
            },
        ))
    }
}

impl LargestSpecimen for EraId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        EraId::new(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for Timestamp {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        const MAX_TIMESTAMP_HUMAN_READABLE: u64 = 253_402_300_799;
        Timestamp::from(MAX_TIMESTAMP_HUMAN_READABLE)
    }
}

impl LargestSpecimen for TimeDiff {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        TimeDiff::from_millis(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for BlockHeaderV1 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        BlockHeaderV1::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            OnceCell::with_value(LargestSpecimen::largest_specimen(estimator, cache)),
        )
    }
}

impl LargestSpecimen for BlockHeaderV2 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        BlockHeaderV2::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            OnceCell::with_value(LargestSpecimen::largest_specimen(estimator, cache)),
        )
    }
}

impl LargestSpecimen for BlockHeader {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let v1 = BlockHeaderV1::largest_specimen(estimator, cache);
        let v2 = BlockHeaderV2::largest_specimen(estimator, cache);

        if estimator.estimate(&v1) > estimator.estimate(&v2) {
            BlockHeader::V1(v1)
        } else {
            BlockHeader::V2(v2)
        }
    }
}

/// A wrapper around `BlockHeader` that implements `LargestSpecimen` without including the era
/// end.
pub(crate) struct BlockHeaderWithoutEraEnd(BlockHeaderV2);

impl BlockHeaderWithoutEraEnd {
    pub(crate) fn into_block_header(self) -> BlockHeader {
        BlockHeader::V2(self.0)
    }
}

impl LargestSpecimen for BlockHeaderWithoutEraEnd {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        BlockHeaderWithoutEraEnd(BlockHeaderV2::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            None,
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            OnceCell::with_value(LargestSpecimen::largest_specimen(estimator, cache)),
        ))
    }
}

impl LargestSpecimen for EraEndV1 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        EraEndV1::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            btree_map_distinct_from_prop(estimator, "validator_count", cache),
        )
    }
}

impl LargestSpecimen for EraEndV2 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        EraEndV2::new(
            vec_prop_specimen(estimator, "validator_count", cache),
            vec_prop_specimen(estimator, "validator_count", cache),
            btree_map_distinct_from_prop(estimator, "validator_count", cache),
            btree_map_distinct_from_prop(estimator, "validator_count", cache),
        )
    }
}

impl LargestSpecimen for InternalEraReport {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        InternalEraReport {
            equivocators: vec_prop_specimen(estimator, "validator_count", cache),
            inactive_validators: vec_prop_specimen(estimator, "validator_count", cache),
        }
    }
}

impl LargestSpecimen for SignedBlockHeader {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        SignedBlockHeader::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for BlockSignatures {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let mut block_signatures = BlockSignaturesV2::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        );
        let sigs = btree_map_distinct_from_prop(estimator, "validator_count", cache);
        sigs.into_iter().for_each(|(public_key, sig)| {
            block_signatures.insert_signature(public_key, sig);
        });
        BlockSignatures::V2(block_signatures)
    }
}

impl LargestSpecimen for BlockV2 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let transfer_hashes = vec![
            TransactionHash::largest_specimen(estimator, cache);
            estimator.parameter::<usize>("max_transfers_per_block")
        ];
        let staking_hashes = vec![
            TransactionHash::largest_specimen(estimator, cache);
            estimator.parameter::<usize>("max_staking_transactions_per_block")
        ];
        let install_upgrade_hashes =
            vec![
                TransactionHash::largest_specimen(estimator, cache);
                estimator.parameter::<usize>("max_install_upgrade_transactions_per_block")
            ];
        let standard_hashes = vec![
            TransactionHash::largest_specimen(estimator, cache);
            estimator
                .parameter::<usize>("max_standard_transactions_per_block")
        ];

        BlockV2::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            transfer_hashes,
            staking_hashes,
            install_upgrade_hashes,
            standard_hashes,
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for Block {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        Block::V2(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for FinalizedBlock {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        FinalizedBlock::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for FinalitySignature {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        FinalitySignature::V2(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for FinalitySignatureV2 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        FinalitySignatureV2::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for FinalitySignatureId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        FinalitySignatureId::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for EraReport<PublicKey> {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        EraReport::new(
            vec_prop_specimen(estimator, "validator_count", cache),
            btree_map_distinct_from_prop(estimator, "validator_count", cache),
            vec_prop_specimen(estimator, "validator_count", cache),
        )
    }
}

impl LargestSpecimen for BlockHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        BlockHash::new(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for ChainNameDigest {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        // ChainNameDigest is fixed size by definition, so any value will do.
        ChainNameDigest::from_chain_name("")
    }
}

// impls for `casper_hashing`, which is technically a foreign crate -- so we put them here.
impl LargestSpecimen for Digest {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        // Hashes are fixed size by definition, so any value will do.
        Digest::hash("")
    }
}

impl LargestSpecimen for BlockPayload {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        // We cannot just use the standard largest specimen for `DeployHashWithApprovals`, as this
        // would cause a quadratic increase in deploys. Instead, we generate one large deploy that
        // contains the number of approvals if they are spread out across the block.

        let large_txn = match Transaction::largest_specimen(estimator, cache) {
            Transaction::Deploy(deploy) => {
                Transaction::Deploy(deploy.with_approvals(btree_set_distinct_from_prop(
                    estimator,
                    "average_approvals_per_transaction_in_block",
                    cache,
                )))
            }
            Transaction::V1(v1) => {
                Transaction::V1(v1.with_approvals(btree_set_distinct_from_prop(
                    estimator,
                    "average_approvals_per_transaction_in_block",
                    cache,
                )))
            }
        };
        let large_txn_hash_with_approvals = TransactionHashWithApprovals::from(&large_txn);

        let transfer_hashes = vec![
            large_txn_hash_with_approvals.clone();
            estimator.parameter::<usize>("max_transfers_per_block")
        ];
        let staking_hashes = vec![
            large_txn_hash_with_approvals.clone();
            estimator.parameter::<usize>("max_staking_transactions_per_block")
        ];
        let install_upgrade_hashes =
            vec![
                large_txn_hash_with_approvals.clone();
                estimator.parameter::<usize>("max_install_upgrade_transactions_per_block")
            ];
        let standard_hashes = vec![
            large_txn_hash_with_approvals;
            estimator
                .parameter::<usize>("max_standard_transactions_per_block")
        ];

        BlockPayload::new(
            transfer_hashes,
            staking_hashes,
            install_upgrade_hashes,
            standard_hashes,
            vec_prop_specimen(estimator, "max_accusations_per_block", cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for RewardedSignatures {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        RewardedSignatures::new(
            std::iter::repeat(LargestSpecimen::largest_specimen(estimator, cache))
                .take(estimator.parameter("signature_rewards_max_delay")),
        )
    }
}

impl LargestSpecimen for SingleBlockRewardedSignatures {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, _cache: &mut Cache) -> Self {
        SingleBlockRewardedSignatures::pack(
            std::iter::repeat(1).take(estimator.parameter("validator_count")),
        )
    }
}

impl LargestSpecimen for DeployHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        DeployHash::new(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for DeployApproval {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        DeployApproval::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl<E> LargeUniqueSequence<E> for DeployApproval
where
    Self: Sized + Ord,
    E: SizeEstimator,
{
    fn large_unique_sequence(estimator: &E, count: usize, cache: &mut Cache) -> BTreeSet<Self> {
        PublicKey::large_unique_sequence(estimator, count, cache)
            .into_iter()
            .map(|public_key| {
                DeployApproval::new(
                    public_key,
                    LargestSpecimen::largest_specimen(estimator, cache),
                )
            })
            .collect()
    }
}

impl LargestSpecimen for DeployHashWithApprovals {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        // Note: This is an upper bound, the actual value is lower. We are keeping the order of
        //       magnitude intact though.
        let max_items = estimator.parameter::<usize>("max_transfers_per_block")
            + estimator.parameter::<usize>("max_standard_per_block");
        DeployHashWithApprovals::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            btree_set_distinct(estimator, max_items, cache),
        )
    }
}

impl LargestSpecimen for Deploy {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        // Note: Deploys have a maximum size enforced on their serialized representation. A deploy
        //       generated here is guaranteed to exceed this maximum size due to the session code
        //       being this maximum size already (see the [`LargestSpecimen`] implementation of
        //       [`ExecutableDeployItem`]). For this reason, we leave `dependencies` and `payment`
        //       small.
        Deploy::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            Default::default(), // See note.
            largest_chain_name(estimator),
            LargestSpecimen::largest_specimen(estimator, cache),
            ExecutableDeployItem::Transfer {
                args: Default::default(), // See note.
            },
            &LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for DeployId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        DeployId::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for DeployApprovalsHash {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        DeployApprovalsHash::compute(&Default::default())
            .expect("empty approvals hash should compute")
    }
}

impl LargestSpecimen for TransactionV1Approval {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        TransactionV1Approval::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl<E> LargeUniqueSequence<E> for TransactionV1Approval
where
    Self: Sized + Ord,
    E: SizeEstimator,
{
    fn large_unique_sequence(estimator: &E, count: usize, cache: &mut Cache) -> BTreeSet<Self> {
        PublicKey::large_unique_sequence(estimator, count, cache)
            .into_iter()
            .map(|public_key| {
                TransactionV1Approval::new(
                    public_key,
                    LargestSpecimen::largest_specimen(estimator, cache),
                )
            })
            .collect()
    }
}

impl LargestSpecimen for TransactionApprovalsHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let deploy_ah =
            TransactionApprovalsHash::Deploy(LargestSpecimen::largest_specimen(estimator, cache));
        let txn_v1_ah =
            TransactionApprovalsHash::V1(LargestSpecimen::largest_specimen(estimator, cache));

        if estimator.estimate(&deploy_ah) >= estimator.estimate(&txn_v1_ah) {
            deploy_ah
        } else {
            txn_v1_ah
        }
    }
}

impl LargestSpecimen for TransactionV1Hash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        TransactionV1Hash::new(LargestSpecimen::largest_specimen(estimator, cache))
    }
}

impl LargestSpecimen for TransactionV1 {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        TransactionV1Builder::new_transfer(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            U512::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
        .unwrap()
        .with_secret_key(&LargestSpecimen::largest_specimen(estimator, cache))
        .with_timestamp(LargestSpecimen::largest_specimen(estimator, cache))
        .with_ttl(LargestSpecimen::largest_specimen(estimator, cache))
        .with_chain_name(largest_chain_name(estimator))
        .with_payment_amount(LargestSpecimen::largest_specimen(estimator, cache))
        .build()
        .unwrap()
    }
}

impl LargestSpecimen for TransactionV1ApprovalsHash {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        TransactionV1ApprovalsHash::compute(&Default::default())
            .expect("empty approvals hash should compute")
    }
}

impl LargestSpecimen for TransactionId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let deploy = TransactionId::new_deploy(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        );
        let v1 = TransactionId::new_v1(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        );

        if estimator.estimate(&deploy) >= estimator.estimate(&v1) {
            deploy
        } else {
            v1
        }
    }
}

impl LargestSpecimen for Transaction {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let deploy = Transaction::Deploy(LargestSpecimen::largest_specimen(estimator, cache));
        let v1 = Transaction::V1(LargestSpecimen::largest_specimen(estimator, cache));

        if estimator.estimate(&deploy) >= estimator.estimate(&v1) {
            deploy
        } else {
            v1
        }
    }
}

impl LargestSpecimen for TransactionHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let deploy_hash =
            TransactionHash::Deploy(LargestSpecimen::largest_specimen(estimator, cache));
        let v1_hash = TransactionHash::V1(LargestSpecimen::largest_specimen(estimator, cache));

        if estimator.estimate(&deploy_hash) >= estimator.estimate(&v1_hash) {
            deploy_hash
        } else {
            v1_hash
        }
    }
}

// EE impls
impl LargestSpecimen for ExecutableDeployItem {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        // `module_bytes` already blows this up to the maximum deploy size, so we use this variant
        // as the largest always and don't need to fill in any args.
        //
        // However, this does not hold true for all encoding schemes: An inefficient encoding can
        // easily, via `RuntimeArgs`, result in a much larger encoded size, e.g. when encoding an
        // array of 1-byte elements in a format that uses string quoting and a delimiter to seperate
        // elements.
        //
        // We compromise by not supporting encodings this inefficient and add 10 * a 32-bit integer
        // as a safety margin for tags and length prefixes.
        let max_size_with_margin =
            estimator.parameter::<i32>("max_transaction_size").max(0) as usize + 10 * 4;

        ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::from(vec_of_largest_specimen(
                estimator,
                max_size_with_margin,
                cache,
            )),
            args: RuntimeArgs::new(),
        }
    }
}

impl LargestSpecimen for U512 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        U512::max_value()
    }
}

impl LargestSpecimen for PackageHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        PackageHash::new([LargestSpecimen::largest_specimen(estimator, cache); KEY_HASH_LENGTH])
    }
}

impl LargestSpecimen for ChunkWithProof {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        ChunkWithProof::new(&[0xFF; Self::CHUNK_SIZE_BYTES], 0)
            .expect("the chunk to be correctly created")
    }
}

impl LargestSpecimen for SecretKey {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        SecretKey::ed25519_from_bytes([u8::MAX; 32]).expect("valid secret key bytes")
    }
}

impl<T: LargestSpecimen> LargestSpecimen for ValidatorMap<T> {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let max_validators = estimator.parameter("validator_count");

        ValidatorMap::from_iter(
            std::iter::repeat_with(|| LargestSpecimen::largest_specimen(estimator, cache))
                .take(max_validators),
        )
    }
}

/// Returns the largest `Message::GetRequest`.
pub(crate) fn largest_get_request<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Message {
    largest_variant::<Message, Tag, _, _>(estimator, |variant| {
        match variant {
            Tag::Transaction => Message::new_get_request::<Transaction>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::LegacyDeploy => Message::new_get_request::<LegacyDeploy>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::Block => Message::new_get_request::<Block>(&LargestSpecimen::largest_specimen(
                estimator, cache,
            )),
            Tag::BlockHeader => Message::new_get_request::<BlockHeader>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::TrieOrChunk => Message::new_get_request::<TrieOrChunk>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::FinalitySignature => Message::new_get_request::<FinalitySignature>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::SyncLeap => Message::new_get_request::<SyncLeap>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::ApprovalsHashes => Message::new_get_request::<ApprovalsHashes>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::BlockExecutionResults => Message::new_get_request::<BlockExecutionResultsOrChunk>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
        }
        .expect("did not expect new_get_request from largest deploy to fail")
    })
}

/// Returns the largest `Message::GetResponse`.
pub(crate) fn largest_get_response<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Message {
    largest_variant::<Message, Tag, _, _>(estimator, |variant| {
        match variant {
            Tag::Transaction => Message::new_get_response::<Transaction>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::LegacyDeploy => Message::new_get_response::<LegacyDeploy>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::Block => Message::new_get_response::<Block>(&LargestSpecimen::largest_specimen(
                estimator, cache,
            )),
            Tag::BlockHeader => Message::new_get_response::<BlockHeader>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::TrieOrChunk => Message::new_get_response::<TrieOrChunk>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::FinalitySignature => Message::new_get_response::<FinalitySignature>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::SyncLeap => Message::new_get_response::<SyncLeap>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::ApprovalsHashes => Message::new_get_response::<ApprovalsHashes>(
                &LargestSpecimen::largest_specimen(estimator, cache),
            ),
            Tag::BlockExecutionResults => {
                Message::new_get_response::<BlockExecutionResultsOrChunk>(
                    &LargestSpecimen::largest_specimen(estimator, cache),
                )
            }
        }
        .expect("did not expect new_get_response from largest deploy to fail")
    })
}

/// Returns the largest string allowed for a chain name.
fn largest_chain_name<E: SizeEstimator>(estimator: &E) -> String {
    string_max_characters(estimator.parameter("network_name_limit"))
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
    let minimum_era_height = estimator.parameter("minimum_era_height");
    let era_duration_ms = TimeDiff::from_millis(estimator.parameter("era_duration_ms"));
    let minimum_round_length_ms =
        TimeDiff::from_millis(estimator.parameter("minimum_round_length_ms"));

    max_rounds_per_era(minimum_era_height, era_duration_ms, minimum_round_length_ms)
        .try_into()
        .expect("to be a valid `usize`")
}

#[cfg(test)]
mod tests {
    use super::Cache;

    #[test]
    fn memoization_cache_simple() {
        let mut cache = Cache::default();

        assert!(cache.get::<u32>().is_none());
        assert!(cache.get::<String>().is_none());

        cache.set::<u32>(1234);
        assert_eq!(cache.get::<u32>(), Some(&1234));

        cache.set::<String>("a string is not copy".to_owned());
        assert_eq!(
            cache.get::<String>().map(String::as_str),
            Some("a string is not copy")
        );
        assert_eq!(cache.get::<u32>(), Some(&1234));

        cache.set::<String>("this should not overwrite".to_owned());
        assert_eq!(
            cache.get::<String>().map(String::as_str),
            Some("a string is not copy")
        );
    }
}
