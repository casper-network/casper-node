//! Specimen support.
//!
//! Structs implementing the specimen trait allow for specific sample instances being created, such
//! as the biggest possible.

use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

use casper_hashing::Digest;
use casper_types::{
    crypto::{PublicKey, PublicKeyDiscriminants, Signature},
    AsymmetricType, EraId, ProtocolVersion, SemVer, SignatureDiscriminants,
};
use serde::Serialize;
use strum::IntoEnumIterator;

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
    /// Like `get_parameter`, but does not accept `None` as an answer.
    ///
    /// ##
    fn require_parameter(&self, name: &'static str) -> i64 {
        self.get_parameter(name)
            .unwrap_or_else(|| panic!("missing parameter \"{}\" for specimen estimation", name))
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

/// Produces the largest variant of a specific `enum` using an estimator and a generation function.
pub(crate) fn largest_variant<T, D, E, F>(estimator: &E, generator: F) -> T
where
    T: Serialize,
    D: IntoEnumIterator,
    E: SizeEstimator,
    F: Fn(D) -> T,
{
    let mut candidates = vec![];
    for variant in D::iter() {
        candidates.push(generator(variant))
    }
    candidates.sort_by_key(|candidate| estimator.estimate(candidate));

    candidates
        .into_iter()
        .next()
        .expect("should have at least one candidate")
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

impl<T> LargestSpecimen for Option<T>
where
    T: LargestSpecimen,
{
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        Some(LargestSpecimen::largest_specimen(estimator))
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
        largest_variant::<Self, PublicKeyDiscriminants, _, _>(estimator, |variant| match variant {
            PublicKeyDiscriminants::System => PublicKey::system(),
            PublicKeyDiscriminants::Ed25519 => PublicKey::ed25519_from_bytes(&[0xFFu8, 32])
                .expect("fixed specimen should be valid Ed25519 public key"),
            PublicKeyDiscriminants::Secp256k1 => PublicKey::secp256k1_from_bytes(&[0xFFu8, 32])
                .expect("fixed specimen should be valid Secp256k1 public key"),
        })
    }
}

impl LargestSpecimen for Signature {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        largest_variant::<Self, SignatureDiscriminants, _, _>(estimator, |variant| match variant {
            SignatureDiscriminants::System => Signature::system(),
            SignatureDiscriminants::Ed25519 => Signature::ed25519([0xFFu8; 64])
                .expect("fixed specimen should be valid Ed25519 signature"),
            SignatureDiscriminants::Secp256k1 => Signature::secp256k1([0xFFu8; 64])
                .expect("fixed specimen should be valid Secp256k1 signature"),
        })
    }
}

impl LargestSpecimen for EraId {
    fn largest_specimen<E: SizeEstimator>(estimator: &E) -> Self {
        EraId::new(LargestSpecimen::largest_specimen(estimator))
    }
}

// impls for `casper_hashing`, which is technically a foreign crate -- so we put them here.
impl LargestSpecimen for Digest {
    fn largest_specimen<E: SizeEstimator>(_estimator: &E) -> Self {
        // Hashes are fixed size by definition, so any value will do.
        Digest::hash("")
    }
}
