use std::{collections::BTreeMap, sync::Arc};

use casper_types::{ChainspecRawBytes, ProtocolVersion};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Complete source documents for the running and installed future chainspecs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Chainspecs {
    /// Source documents loaded by the running node, including optional companion files.
    pub current: Arc<ChainspecRawBytes>,
    /// Installed future source documents, keyed and ordered by protocol version.
    pub future: BTreeMap<ProtocolVersion, Arc<ChainspecRawBytes>>,
}

/// Chainspec discovery availability, independent of status and upgrade selection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ChainspecStatus {
    /// Complete, atomically refreshed chainspec source documents.
    Available(Chainspecs),
    /// Installed chainspec source documents could not be read or identified.
    Unavailable {
        /// Diagnostic for the invalid installed chainspec.
        error: String,
    },
}

impl From<Result<Chainspecs, String>> for ChainspecStatus {
    fn from(result: Result<Chainspecs, String>) -> Self {
        match result {
            Ok(chainspecs) => Self::Available(chainspecs),
            Err(error) => Self::Unavailable { error },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(bytes: &[u8]) -> Arc<ChainspecRawBytes> {
        Arc::new(ChainspecRawBytes::new(bytes.to_vec().into(), None, None))
    }

    #[test]
    fn chainspec_status_roundtrips_complete_source_documents() {
        let current = Arc::new(ChainspecRawBytes::new(
            b"# complete chainspec source\n".to_vec().into(),
            Some(b"# accounts\n".to_vec().into()),
            Some(b"# state\n".to_vec().into()),
        ));
        let status = ChainspecStatus::Available(Chainspecs {
            current,
            future: BTreeMap::from([
                (ProtocolVersion::from_parts(10, 0, 0), source(b"# last\n")),
                (ProtocolVersion::from_parts(2, 2, 0), source(b"# next\n")),
            ]),
        });
        let json = serde_json::to_value(&status).unwrap();
        assert!(json["current"]["chainspec_bytes"].is_string());
        assert!(json["future"]["2.2.0"]["chainspec_bytes"].is_string());
        assert_eq!(
            serde_json::from_value::<ChainspecStatus>(json).unwrap(),
            status
        );
    }
}
