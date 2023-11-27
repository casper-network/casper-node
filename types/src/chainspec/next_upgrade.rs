use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    ActivationPoint, ProtocolConfig, ProtocolVersion,
};

/// Information about the next protocol upgrade.
#[derive(PartialEq, Eq, DataSize, Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct NextUpgrade {
    activation_point: ActivationPoint,
    #[data_size(skip)]
    #[schemars(with = "String")]
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
