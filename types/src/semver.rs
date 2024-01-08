use alloc::vec::Vec;
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
    num::ParseIntError,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, Error, FromBytes, ToBytes, U32_SERIALIZED_LENGTH};

/// Length of SemVer when serialized
pub const SEM_VER_SERIALIZED_LENGTH: usize = 3 * U32_SERIALIZED_LENGTH;

/// A struct for semantic versioning.
#[derive(
    Copy, Clone, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct SemVer {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version.
    pub patch: u32,
}

impl SemVer {
    /// Version 1.0.0.
    pub const V1_0_0: SemVer = SemVer {
        major: 1,
        minor: 0,
        patch: 0,
    };

    /// Constructs a new `SemVer` from the given semver parts.
    pub const fn new(major: u32, minor: u32, patch: u32) -> SemVer {
        SemVer {
            major,
            minor,
            patch,
        }
    }

    /// Return `true` if the two versions have compatible major numbers.
    pub fn is_major_compatible(self, other: SemVer) -> bool {
        if self.major == 0 || other.major == 0 {
            // SEMVER requires that major version 0 be considered unstable,
            // so a precise version match is required.
            self == other
        } else {
            self.major == other.major
        }
    }
}

impl ToBytes for SemVer {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.major.to_bytes()?);
        ret.append(&mut self.minor.to_bytes()?);
        ret.append(&mut self.patch.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        SEM_VER_SERIALIZED_LENGTH
    }
}

impl FromBytes for SemVer {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (major, rem): (u32, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (minor, rem): (u32, &[u8]) = FromBytes::from_bytes(rem)?;
        let (patch, rem): (u32, &[u8]) = FromBytes::from_bytes(rem)?;
        Ok((SemVer::new(major, minor, patch), rem))
    }
}

impl Display for SemVer {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parsing error when creating a SemVer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseSemVerError {
    /// Invalid version format.
    InvalidVersionFormat,
    /// Error parsing an integer.
    ParseIntError(ParseIntError),
}

impl Display for ParseSemVerError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            ParseSemVerError::InvalidVersionFormat => formatter.write_str("invalid version format"),
            ParseSemVerError::ParseIntError(error) => error.fmt(formatter),
        }
    }
}

impl From<ParseIntError> for ParseSemVerError {
    fn from(error: ParseIntError) -> ParseSemVerError {
        ParseSemVerError::ParseIntError(error)
    }
}

impl TryFrom<&str> for SemVer {
    type Error = ParseSemVerError;
    fn try_from(value: &str) -> Result<SemVer, Self::Error> {
        let tokens: Vec<&str> = value.split('.').collect();
        if tokens.len() != 3 {
            return Err(ParseSemVerError::InvalidVersionFormat);
        }

        Ok(SemVer {
            major: tokens[0].parse()?,
            minor: tokens[1].parse()?,
            patch: tokens[2].parse()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::TryInto;

    #[test]
    fn should_compare_semver_versions() {
        assert!(SemVer::new(0, 0, 0) < SemVer::new(1, 2, 3));
        assert!(SemVer::new(1, 1, 0) < SemVer::new(1, 2, 0));
        assert!(SemVer::new(1, 0, 0) < SemVer::new(1, 2, 0));
        assert!(SemVer::new(1, 0, 0) < SemVer::new(1, 2, 3));
        assert!(SemVer::new(1, 2, 0) < SemVer::new(1, 2, 3));
        assert!(SemVer::new(1, 2, 3) == SemVer::new(1, 2, 3));
        assert!(SemVer::new(1, 2, 3) >= SemVer::new(1, 2, 3));
        assert!(SemVer::new(1, 2, 3) <= SemVer::new(1, 2, 3));
        assert!(SemVer::new(2, 0, 0) >= SemVer::new(1, 99, 99));
        assert!(SemVer::new(2, 0, 0) > SemVer::new(1, 99, 99));
    }

    #[test]
    fn parse_from_string() {
        let ver1: SemVer = "100.20.3".try_into().expect("should parse");
        assert_eq!(ver1, SemVer::new(100, 20, 3));
        let ver2: SemVer = "0.0.1".try_into().expect("should parse");
        assert_eq!(ver2, SemVer::new(0, 0, 1));

        assert!(SemVer::try_from("1.a.2.3").is_err());
        assert!(SemVer::try_from("1. 2.3").is_err());
        assert!(SemVer::try_from("12345124361461.0.1").is_err());
        assert!(SemVer::try_from("1.2.3.4").is_err());
        assert!(SemVer::try_from("1.2").is_err());
        assert!(SemVer::try_from("1").is_err());
        assert!(SemVer::try_from("0").is_err());
    }
}
