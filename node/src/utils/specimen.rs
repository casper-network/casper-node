//! Specimen support.
//!
//! Structs implementing the specimen trait allow for specific sample instances being created, such
//! as the biggest possible.

use core::convert::TryInto;
use std::{
    any::{Any, TypeId},
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::TryFrom,
    iter::FromIterator,
    net::{Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::{Arc, Mutex},
};

use casper_execution_engine::core::engine_state::{
    executable_deploy_item::ExecutableDeployItemDiscriminants, ExecutableDeployItem,
};
use casper_hashing::{ChunkWithProof, Digest};
use casper_types::{
    bytesrepr::Bytes,
    crypto::{sign, PublicKey, Signature},
    AsymmetricType, ContractHash, ContractPackageHash, EraId, ProtocolVersion, RuntimeArgs,
    SecretKey, SemVer, TimeDiff, Timestamp, KEY_HASH_LENGTH, U512,
};
use either::Either;
use once_cell::sync::Lazy;
use serde::Serialize;
use strum::{EnumIter, IntoEnumIterator};

use crate::{
    components::{
        consensus::{max_rounds_per_era, utils::ValidatorMap, EraReport},
        fetcher::Tag,
    },
    protocol::Message,
    types::{
        Approval, ApprovalsHash, ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHash,
        BlockHeader, BlockPayload, Deploy, DeployHashWithApprovals, DeployId, FinalitySignature,
        FinalitySignatureId, FinalizedBlock, LegacyDeploy, SyncLeap, TrieOrChunk,
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
        self.get_raw::<T>()
            .get(0)
            .map(|box_any| box_any.downcast_ref::<T>().expect("cache corrupted"))
    }

    /// Sets the memoized instance if not already set.
    ///
    /// Returns a reference to the memoized instance. Note that this may be an instance other than
    /// the passed in `item`, if the cache entry was not empty before/
    pub(crate) fn set<T: Any>(&mut self, item: T) -> &T {
        let items = self.get_raw::<T>();
        if items.is_empty() {
            let boxed_item: Box<dyn Any> = Box::new(item);
            items.push(boxed_item);
        }
        self.get::<T>().expect("should not be empty")
    }

    /// Get or insert the vector storing item instances.
    fn get_raw<T: Any>(&mut self) -> &mut Vec<Box<dyn Any>> {
        self.items.entry(TypeId::of::<T>()).or_default()
    }
}

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

    /// Require a parameter, cast into a boolean.
    ///
    /// See `require_parameter` for details. Will return `false` if the stored value is `0`,
    /// otherwise `true`.
    ///
    /// ## Panics
    ///
    /// Same as `require_parameter`.
    fn require_parameter_bool(&self, name: &'static str) -> bool {
        self.require_parameter::<i64>(name) != 0
    }

    /// An identifier for this specific estimator, used for memoization.
    fn key(&self) -> &str;
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
    let mut count = estimator.require_parameter(parameter_name);
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
    let mut count = estimator.require_parameter(parameter_name);
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
    let mut count = estimator.require_parameter(parameter_name);
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
    fn large_unique_sequence(estimator: &E, count: usize, _cache: &mut Cache) -> BTreeSet<Self> {
        static MEMOIZED: Lazy<Mutex<HashMap<String, Vec<PublicKey>>>> =
            Lazy::new(|| Mutex::new(HashMap::new()));

        let mut guard = MEMOIZED.lock().expect("memoization cache disappeared");

        let data_vec = guard.entry(estimator.key().to_owned()).or_default();

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
                // We take advantange of two things here:
                //
                // 1. The required seed bytes for Ed25519 and Secp256k1 are both the same length of
                //    32 bytes.
                // 2. While Secp256k1 does not allow the most trivial seed bytes of 0x00..0001, a
                //    a hash function output seems to satisfay it, and our current hashing scheme
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
            data_vec.push(generate_key(estimator, seed));
        }

        debug_assert!(data_vec.len() >= count);
        let output_set: BTreeSet<Self> = data_vec[..count].iter().cloned().collect();
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

/// Memoize a value using a local static variable.
#[macro_export]
macro_rules! memoize {
    ($ty:ty, $estimator:expr, $blk:block) => {
        #[allow(unused_qualifications)]
        {
            static MEMOIZED: ::once_cell::sync::Lazy<
                ::std::sync::Mutex<std::collections::HashMap<String, $ty>>,
            > = ::once_cell::sync::Lazy::new(|| {
                ::std::sync::Mutex::new(::std::collections::HashMap::new())
            });
            MEMOIZED
                .lock()
                .expect("memoization cache disappeared")
                .entry($estimator.key().to_owned())
                .or_insert_with(|| $blk)
                .clone()
        }
    };
}

impl LargestSpecimen for Signature {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, _cache: &mut Cache) -> Self {
        // Note: We do not use strum generated discriminator enums for the signature, as we do not
        //       want to make `strum` a direct dependency of `casper-types`, to keep its size down.
        #[derive(Debug, Copy, Clone, EnumIter)]
        enum SignatureDiscriminants {
            System,
            Ed25519,
            Secp256k1,
        }

        memoize!(Signature, estimator, {
            largest_variant::<Self, SignatureDiscriminants, _, _>(
                estimator,
                |variant| match variant {
                    SignatureDiscriminants::System => Signature::system(),
                    SignatureDiscriminants::Ed25519 => {
                        let ed25519_sec = &SecretKey::generate_ed25519().expect("a correct secret");

                        sign([0_u8], ed25519_sec, &ed25519_sec.into())
                    }
                    SignatureDiscriminants::Secp256k1 => {
                        let secp256k1_sec =
                            &SecretKey::generate_secp256k1().expect("a correct secret");

                        sign([0_u8], secp256k1_sec, &secp256k1_sec.into())
                    }
                },
            )
        })
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

impl LargestSpecimen for Block {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        Block::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            Some(btree_map_distinct_from_prop(
                estimator,
                "validator_count",
                cache,
            )),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
        .expect("did not expect largest speciment creation of block to fail")
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
        FinalitySignature::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for FinalitySignatureId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        FinalitySignatureId {
            block_hash: LargestSpecimen::largest_specimen(estimator, cache),
            era_id: LargestSpecimen::largest_specimen(estimator, cache),
            public_key: LargestSpecimen::largest_specimen(estimator, cache),
        }
    }
}

impl LargestSpecimen for EraReport<PublicKey> {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        EraReport {
            equivocators: vec_prop_specimen(estimator, "validator_count", cache),
            rewards: btree_map_distinct_from_prop(estimator, "validator_count", cache),
            inactive_validators: vec_prop_specimen(estimator, "validator_count", cache),
        }
    }
}

impl LargestSpecimen for BlockHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        BlockHash::new(LargestSpecimen::largest_specimen(estimator, cache))
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
        // We cannot just use the standard largest speciment for `DeployHashWithApprovals`, as this
        // would cause a quadratic increase in deploys. Instead, we generate one large deploy that
        // contains the number of approvals if they are spread out across the block.

        let large_deploy = Deploy::largest_specimen(estimator, cache).with_approvals(
            btree_set_distinct_from_prop(estimator, "average_approvals_per_deploy_in_block", cache),
        );
        let large_deploy_hash_with_approvals = DeployHashWithApprovals::from(&large_deploy);

        let deploys = vec![
            large_deploy_hash_with_approvals.clone();
            estimator.require_parameter::<usize>("max_deploys_per_block")
        ];
        let transfers = vec![
            large_deploy_hash_with_approvals;
            estimator.require_parameter::<usize>("max_transfers_per_block")
        ];

        BlockPayload::new(
            deploys,
            transfers,
            vec_prop_specimen(estimator, "max_accusations_per_block", cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl LargestSpecimen for DeployHashWithApprovals {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        // Note: This is an upper bound, the actual value is lower. We are keeping the order of
        //       magnitude intact though.
        let max_items = estimator.require_parameter::<usize>("max_deploys_per_block")
            + estimator.require_parameter::<usize>("max_transfers_per_block");
        DeployHashWithApprovals::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            btree_set_distinct(estimator, max_items, cache),
        )
    }
}

impl LargestSpecimen for Deploy {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        Deploy::new(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
            vec![/* Legacy field, always empty */],
            largest_chain_name(estimator),
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
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

impl LargestSpecimen for Approval {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        Approval::from_parts(
            LargestSpecimen::largest_specimen(estimator, cache),
            LargestSpecimen::largest_specimen(estimator, cache),
        )
    }
}

impl<E> LargeUniqueSequence<E> for Approval
where
    Self: Sized + Ord,
    E: SizeEstimator,
{
    fn large_unique_sequence(estimator: &E, count: usize, cache: &mut Cache) -> BTreeSet<Self> {
        PublicKey::large_unique_sequence(estimator, count, cache)
            .into_iter()
            .map(|pk| Approval::from_parts(pk, LargestSpecimen::largest_specimen(estimator, cache)))
            .collect()
    }
}

impl LargestSpecimen for ApprovalsHash {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        ApprovalsHash::compute(&Default::default()).expect("empty approvals hash should compute")
    }
}

// EE impls
impl LargestSpecimen for ExecutableDeployItem {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        largest_variant::<Self, ExecutableDeployItemDiscriminants, _, _>(estimator, |variant| {
            match variant {
                ExecutableDeployItemDiscriminants::ModuleBytes => {
                    ExecutableDeployItem::ModuleBytes {
                        module_bytes: Bytes::from(vec_prop_specimen(
                            estimator,
                            "module_bytes",
                            cache,
                        )),
                        args: LargestSpecimen::largest_specimen(estimator, cache),
                    }
                }
                ExecutableDeployItemDiscriminants::StoredContractByHash => {
                    ExecutableDeployItem::StoredContractByHash {
                        hash: LargestSpecimen::largest_specimen(estimator, cache),
                        entry_point: largest_contract_entry_point(estimator),
                        args: LargestSpecimen::largest_specimen(estimator, cache),
                    }
                }
                ExecutableDeployItemDiscriminants::StoredContractByName => {
                    ExecutableDeployItem::StoredContractByName {
                        name: largest_contract_name(estimator),
                        entry_point: largest_contract_entry_point(estimator),
                        args: LargestSpecimen::largest_specimen(estimator, cache),
                    }
                }
                ExecutableDeployItemDiscriminants::StoredVersionedContractByHash => {
                    ExecutableDeployItem::StoredVersionedContractByHash {
                        hash: LargestSpecimen::largest_specimen(estimator, cache),
                        version: LargestSpecimen::largest_specimen(estimator, cache),
                        entry_point: largest_contract_entry_point(estimator),
                        args: LargestSpecimen::largest_specimen(estimator, cache),
                    }
                }
                ExecutableDeployItemDiscriminants::StoredVersionedContractByName => {
                    ExecutableDeployItem::StoredVersionedContractByName {
                        name: largest_contract_name(estimator),
                        version: LargestSpecimen::largest_specimen(estimator, cache),
                        entry_point: largest_contract_entry_point(estimator),
                        args: LargestSpecimen::largest_specimen(estimator, cache),
                    }
                }
                ExecutableDeployItemDiscriminants::Transfer => ExecutableDeployItem::Transfer {
                    args: LargestSpecimen::largest_specimen(estimator, cache),
                },
            }
        })
    }
}

impl LargestSpecimen for U512 {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        U512::max_value()
    }
}

impl LargestSpecimen for ContractHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        ContractHash::new([LargestSpecimen::largest_specimen(estimator, cache); KEY_HASH_LENGTH])
    }
}

impl LargestSpecimen for ContractPackageHash {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        ContractPackageHash::new(
            [LargestSpecimen::largest_specimen(estimator, cache); KEY_HASH_LENGTH],
        )
    }
}

impl LargestSpecimen for RuntimeArgs {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        Default::default()
    }
}

impl LargestSpecimen for ChunkWithProof {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        ChunkWithProof::new(&[0xFF; 8 * 1024 * 1024], 0).expect("the chunk to be correctly created")
    }
}

impl LargestSpecimen for SecretKey {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E, _cache: &mut Cache) -> Self {
        SecretKey::ed25519_from_bytes([u8::MAX; 32]).expect("valid secret key bytes")
    }
}

impl<T: LargestSpecimen> LargestSpecimen for ValidatorMap<T> {
    fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
        let max_validators = estimator.require_parameter("validator_count");

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
            Tag::Deploy => Message::new_get_request::<Deploy>(&LargestSpecimen::largest_specimen(
                estimator, cache,
            )),
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
            Tag::Deploy => Message::new_get_response::<Deploy>(&LargestSpecimen::largest_specimen(
                estimator, cache,
            )),
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
