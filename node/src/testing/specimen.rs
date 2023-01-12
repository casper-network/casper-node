//! Specimen support.
//!
//! Structs implementing the specimen trait allow for specific sample instances being created, such
//! as the biggest possible.

use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

use casper_types::{ProtocolVersion, SemVer};
use serde::Serialize;

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
