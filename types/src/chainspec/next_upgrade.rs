use std::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    ActivationPoint, ProtocolConfig, ProtocolVersion,
};

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

/// Information about the next protocol upgrade.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[derive(PartialEq, Eq, Debug, Serialize, Deserialize, Clone)]
pub struct NextUpgrade {
    activation_point: ActivationPoint,
    protocol_version: ProtocolVersion,
}

impl NextUpgrade {
    /// Creates a new `NextUpgrade`.
    pub fn new(activation_point: ActivationPoint, protocol_version: ProtocolVersion) -> Self {
        NextUpgrade {
            activation_point,
            protocol_version,
        }
    }

    /// Returns the activation point of the next upgrade.
    pub fn activation_point(&self) -> ActivationPoint {
        self.activation_point
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            activation_point: ActivationPoint::random(rng),
            protocol_version: ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen()),
        }
    }
}

impl From<ProtocolConfig> for NextUpgrade {
    fn from(protocol_config: ProtocolConfig) -> Self {
        NextUpgrade {
            activation_point: protocol_config.activation_point,
            protocol_version: protocol_config.version,
        }
    }
}

impl Display for NextUpgrade {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "next upgrade to {} at start of era {}",
            self.protocol_version,
            self.activation_point.era_id()
        )
    }
}

impl ToBytes for NextUpgrade {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.activation_point.write_bytes(writer)?;
        self.protocol_version.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.activation_point.serialized_length() + self.protocol_version.serialized_length()
    }
}

impl FromBytes for NextUpgrade {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (activation_point, remainder) = ActivationPoint::from_bytes(bytes)?;
        let (protocol_version, remainder) = ProtocolVersion::from_bytes(remainder)?;
        Ok((
            NextUpgrade {
                activation_point,
                protocol_version,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = NextUpgrade::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
