use alloc::vec::Vec;
use core::fmt;

use datasize::DataSize;
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{Error, FromBytes, ToBytes},
    SemVer,
};

/// A newtype wrapping a [`SemVer`] which represents a Casper Platform protocol version.
#[derive(
    Copy,
    Clone,
    DataSize,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub struct ProtocolVersion(SemVer);

/// The result of [`ProtocolVersion::check_next_version`].
#[derive(Debug, PartialEq, Eq)]
pub enum VersionCheckResult {
    /// Upgrade possible.
    Valid {
        /// Is this a major protocol version upgrade?
        is_major_version: bool,
    },
    /// Upgrade is invalid.
    Invalid,
}

impl VersionCheckResult {
    /// Checks if given version result is invalid.
    ///
    /// Invalid means that a given version can not be followed.
    pub fn is_invalid(&self) -> bool {
        matches!(self, VersionCheckResult::Invalid)
    }

    /// Checks if given version is a major protocol version upgrade.
    pub fn is_major_version(&self) -> bool {
        match self {
            VersionCheckResult::Valid { is_major_version } => *is_major_version,
            VersionCheckResult::Invalid => false,
        }
    }
}

impl ProtocolVersion {
    /// Version 1.0.0.
    pub const V1_0_0: ProtocolVersion = ProtocolVersion(SemVer {
        major: 1,
        minor: 0,
        patch: 0,
    });

    /// Constructs a new `ProtocolVersion` from `version`.
    pub const fn new(version: SemVer) -> ProtocolVersion {
        ProtocolVersion(version)
    }

    /// Constructs a new `ProtocolVersion` from the given semver parts.
    pub const fn from_parts(major: u32, minor: u32, patch: u32) -> ProtocolVersion {
        let sem_ver = SemVer::new(major, minor, patch);
        Self::new(sem_ver)
    }

    /// Returns the inner [`SemVer`].
    pub fn value(&self) -> SemVer {
        self.0
    }

    /// Checks if next version can be followed.
    pub fn check_next_version(&self, next: &ProtocolVersion) -> VersionCheckResult {
        if next.0.major < self.0.major || next.0.major > self.0.major + 1 {
            // Protocol major versions should not go backwards and should increase monotonically by
            // 1.
            return VersionCheckResult::Invalid;
        }

        if next.0.major == self.0.major.saturating_add(1) {
            // A major version increase resets both the minor and patch versions to ( 0.0 ).
            if next.0.minor != 0 || next.0.patch != 0 {
                return VersionCheckResult::Invalid;
            }
            return VersionCheckResult::Valid {
                is_major_version: true,
            };
        }

        // Covers the equal major versions
        debug_assert_eq!(next.0.major, self.0.major);

        if next.0.minor < self.0.minor || next.0.minor > self.0.minor + 1 {
            // Protocol minor versions should increase monotonically by 1 within the same major
            // version and should not go backwards.
            return VersionCheckResult::Invalid;
        }

        if next.0.minor == self.0.minor + 1 {
            // A minor version increase resets the patch version to ( 0 ).
            if next.0.patch != 0 {
                return VersionCheckResult::Invalid;
            }
            return VersionCheckResult::Valid {
                is_major_version: false,
            };
        }

        // Code belows covers equal minor versions
        debug_assert_eq!(next.0.minor, self.0.minor);

        // Protocol patch versions should increase monotonically but can be skipped.
        if next.0.patch <= self.0.patch {
            return VersionCheckResult::Invalid;
        }

        VersionCheckResult::Valid {
            is_major_version: false,
        }
    }

    /// Checks if given protocol version is compatible with current one.
    ///
    /// Two protocol versions with different major version are considered to be incompatible.
    pub fn is_compatible_with(&self, version: &ProtocolVersion) -> bool {
        self.0.major == version.0.major
    }
}

impl ToBytes for ProtocolVersion {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.value().to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.value().serialized_length()
    }
}

impl FromBytes for ProtocolVersion {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (version, rem) = SemVer::from_bytes(bytes)?;
        let protocol_version = ProtocolVersion::new(version);
        Ok((protocol_version, rem))
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl JsonSchema for ProtocolVersion {
    fn schema_name() -> String {
        String::from("ProtocolVersion")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some("Protocol version of the network".to_string());
        schema_object.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SemVer;

    #[test]
    fn should_follow_version_with_optional_code() {
        let value = VersionCheckResult::Valid {
            is_major_version: false,
        };
        assert!(!value.is_invalid());
        assert!(!value.is_major_version());
    }

    #[test]
    fn should_follow_version_with_required_code() {
        let value = VersionCheckResult::Valid {
            is_major_version: true,
        };
        assert!(!value.is_invalid());
        assert!(value.is_major_version());
    }

    #[test]
    fn should_not_follow_version_with_invalid_code() {
        let value = VersionCheckResult::Invalid;
        assert!(value.is_invalid());
        assert!(!value.is_major_version());
    }

    #[test]
    fn should_be_able_to_get_instance() {
        let initial_value = SemVer::new(1, 0, 0);
        let item = ProtocolVersion::new(initial_value);
        assert_eq!(initial_value, item.value(), "should have equal value")
    }

    #[test]
    fn should_be_able_to_compare_two_instances() {
        let lhs = ProtocolVersion::new(SemVer::new(1, 0, 0));
        let rhs = ProtocolVersion::new(SemVer::new(1, 0, 0));
        assert_eq!(lhs, rhs, "should be equal");
        let rhs = ProtocolVersion::new(SemVer::new(2, 0, 0));
        assert_ne!(lhs, rhs, "should not be equal")
    }

    #[test]
    fn should_be_able_to_default() {
        let defaulted = ProtocolVersion::default();
        let expected = ProtocolVersion::new(SemVer::new(0, 0, 0));
        assert_eq!(defaulted, expected, "should be equal")
    }

    #[test]
    fn should_be_able_to_compare_relative_value() {
        let lhs = ProtocolVersion::new(SemVer::new(2, 0, 0));
        let rhs = ProtocolVersion::new(SemVer::new(1, 0, 0));
        assert!(lhs > rhs, "should be gt");
        let rhs = ProtocolVersion::new(SemVer::new(2, 0, 0));
        assert!(lhs >= rhs, "should be gte");
        assert!(lhs <= rhs, "should be lte");
        let lhs = ProtocolVersion::new(SemVer::new(1, 0, 0));
        assert!(lhs < rhs, "should be lt");
    }

    #[test]
    fn should_follow_major_version_upgrade() {
        // If the upgrade protocol version is lower than or the same as EE's current in-use protocol
        // version the upgrade is rejected and an error is returned; this includes the special case
        // of a defaulted protocol version ( 0.0.0 ).
        let prev = ProtocolVersion::new(SemVer::new(1, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(2, 0, 0));
        assert!(
            prev.check_next_version(&next).is_major_version(),
            "should be major version"
        );
    }

    #[test]
    fn should_reject_if_major_version_decreases() {
        let prev = ProtocolVersion::new(SemVer::new(10, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(9, 0, 0));
        // Major version must not decrease ...
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
    }

    #[test]
    fn should_check_follows_minor_version_upgrade() {
        // [major version] may remain the same in the case of a minor or patch version increase.

        // Minor version must not decrease within the same major version
        let prev = ProtocolVersion::new(SemVer::new(1, 1, 0));
        let next = ProtocolVersion::new(SemVer::new(1, 2, 0));

        let value = prev.check_next_version(&next);
        assert!(!value.is_invalid(), "should be valid");
        assert!(!value.is_major_version(), "should not be a major version");
    }

    #[test]
    fn should_check_if_minor_bump_resets_patch() {
        // A minor version increase resets the patch version to ( 0 ).
        let prev = ProtocolVersion::new(SemVer::new(1, 2, 0));
        let next = ProtocolVersion::new(SemVer::new(1, 3, 1));
        // wrong - patch version should be reset for minor version increase
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);

        let prev = ProtocolVersion::new(SemVer::new(1, 20, 42));
        let next = ProtocolVersion::new(SemVer::new(1, 30, 43));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
    }

    #[test]
    fn should_check_if_major_resets_minor_and_patch() {
        // A major version increase resets both the minor and patch versions to ( 0.0 ).
        let prev = ProtocolVersion::new(SemVer::new(1, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(2, 1, 0));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid); // wrong - major increase should reset minor

        let next = ProtocolVersion::new(SemVer::new(2, 0, 1));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid); // wrong - major increase should reset patch

        let next = ProtocolVersion::new(SemVer::new(2, 1, 1));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
        // wrong - major
        // increase
        // should reset
        // minor and patch
    }

    #[test]
    fn should_reject_patch_version_rollback() {
        // Patch version must not decrease or remain the same within the same major and minor
        // version pair, but may skip.
        let prev = ProtocolVersion::new(SemVer::new(1, 0, 42));
        let next = ProtocolVersion::new(SemVer::new(1, 0, 41));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
        let next = ProtocolVersion::new(SemVer::new(1, 0, 13));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
    }

    #[test]
    fn should_accept_patch_version_update_with_optional_code() {
        let prev = ProtocolVersion::new(SemVer::new(1, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(1, 0, 1));
        let value = prev.check_next_version(&next);
        assert!(!value.is_invalid(), "should be valid");
        assert!(!value.is_major_version(), "should not be a major version");

        let prev = ProtocolVersion::new(SemVer::new(1, 0, 8));
        let next = ProtocolVersion::new(SemVer::new(1, 0, 42));
        let value = prev.check_next_version(&next);
        assert!(!value.is_invalid(), "should be valid");
        assert!(!value.is_major_version(), "should not be a major version");
    }

    #[test]
    fn should_accept_minor_version_update_with_optional_code() {
        // installer is optional for minor bump
        let prev = ProtocolVersion::new(SemVer::new(1, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(1, 1, 0));
        let value = prev.check_next_version(&next);
        assert!(!value.is_invalid(), "should be valid");
        assert!(!value.is_major_version(), "should not be a major version");

        let prev = ProtocolVersion::new(SemVer::new(3, 98, 0));
        let next = ProtocolVersion::new(SemVer::new(3, 99, 0));
        let value = prev.check_next_version(&next);
        assert!(!value.is_invalid(), "should be valid");
        assert!(!value.is_major_version(), "should not be a major version");
    }

    #[test]
    fn should_not_skip_minor_version_within_major_version() {
        // minor can be updated only by 1
        let prev = ProtocolVersion::new(SemVer::new(1, 1, 0));

        let next = ProtocolVersion::new(SemVer::new(1, 3, 0));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);

        let next = ProtocolVersion::new(SemVer::new(1, 7, 0));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
    }

    #[test]
    fn should_reset_minor_and_patch_on_major_bump() {
        // no upgrade - minor resets patch
        let prev = ProtocolVersion::new(SemVer::new(1, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(2, 1, 1));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);

        let prev = ProtocolVersion::new(SemVer::new(1, 1, 1));
        let next = ProtocolVersion::new(SemVer::new(2, 2, 3));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
    }

    #[test]
    fn should_allow_code_on_major_update() {
        // major upgrade requires installer to be present
        let prev = ProtocolVersion::new(SemVer::new(1, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(2, 0, 0));
        assert!(
            prev.check_next_version(&next).is_major_version(),
            "should be major version"
        );

        let prev = ProtocolVersion::new(SemVer::new(2, 99, 99));
        let next = ProtocolVersion::new(SemVer::new(3, 0, 0));
        assert!(
            prev.check_next_version(&next).is_major_version(),
            "should be major version"
        );
    }

    #[test]
    fn should_not_skip_major_version() {
        // can bump only by 1
        let prev = ProtocolVersion::new(SemVer::new(1, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(3, 0, 0));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
    }

    #[test]
    fn should_reject_major_version_rollback() {
        // can bump forward
        let prev = ProtocolVersion::new(SemVer::new(2, 0, 0));
        let next = ProtocolVersion::new(SemVer::new(0, 0, 0));
        assert_eq!(prev.check_next_version(&next), VersionCheckResult::Invalid);
    }

    #[test]
    fn should_check_same_version_is_invalid() {
        for ver in &[
            ProtocolVersion::from_parts(1, 0, 0),
            ProtocolVersion::from_parts(1, 2, 0),
            ProtocolVersion::from_parts(1, 2, 3),
        ] {
            assert_eq!(ver.check_next_version(&ver), VersionCheckResult::Invalid);
        }
    }

    #[test]
    fn should_not_be_compatible_with_different_major_version() {
        let current = ProtocolVersion::from_parts(1, 2, 3);
        let other = ProtocolVersion::from_parts(2, 5, 6);
        assert!(!current.is_compatible_with(&other));

        let current = ProtocolVersion::from_parts(1, 0, 0);
        let other = ProtocolVersion::from_parts(2, 0, 0);
        assert!(!current.is_compatible_with(&other));
    }

    #[test]
    fn should_be_compatible_with_equal_major_version_backwards() {
        let current = ProtocolVersion::from_parts(1, 99, 99);
        let other = ProtocolVersion::from_parts(1, 0, 0);
        assert!(current.is_compatible_with(&other));
    }

    #[test]
    fn should_be_compatible_with_equal_major_version_forwards() {
        let current = ProtocolVersion::from_parts(1, 0, 0);
        let other = ProtocolVersion::from_parts(1, 99, 99);
        assert!(current.is_compatible_with(&other));
    }
}
