use alloc::vec::Vec;
use core::fmt::{self, Debug, Display, Formatter};

use super::{
    serialization::transaction_target::*, TransactionInvocationTarget, TransactionRuntime,
};
use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{Rng, RngCore};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The execution target of a [`Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Execution target of a Transaction.")
)]
#[serde(deny_unknown_fields)]
pub enum TransactionTarget {
    /// The execution target is a native operation (e.g. a transfer).
    Native,
    /// The execution target is a stored entity or package.
    Stored {
        /// The identifier of the stored execution target.
        id: TransactionInvocationTarget,
        /// The execution runtime to use.
        runtime: TransactionRuntime,
    },
    /// The execution target is the included module bytes, i.e. compiled Wasm.
    Session {
        /// The compiled Wasm.
        module_bytes: Bytes,
        /// The execution runtime to use.
        runtime: TransactionRuntime,
    },
}

impl TransactionTarget {
    /// Returns a new `TransactionTarget::Native`.
    pub fn new_native() -> Self {
        TransactionTarget::Native
    }

    /// Returns a new `TransactionTarget::Stored`.
    pub fn new_stored(id: TransactionInvocationTarget, runtime: TransactionRuntime) -> Self {
        TransactionTarget::Stored { id, runtime }
    }

    /// Returns a new `TransactionTarget::Session`.
    pub fn new_session(module_bytes: Bytes, runtime: TransactionRuntime) -> Self {
        TransactionTarget::Session {
            module_bytes,
            runtime,
        }
    }

    /// Returns a random `TransactionTarget`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            NATIVE_TAG => TransactionTarget::Native,
            STORED_TAG => TransactionTarget::new_stored(
                TransactionInvocationTarget::random(rng),
                TransactionRuntime::VmCasperV1,
            ),
            SESSION_TAG => {
                let mut buffer = vec![0u8; rng.gen_range(0..100)];
                rng.fill_bytes(buffer.as_mut());
                TransactionTarget::new_session(Bytes::from(buffer), TransactionRuntime::VmCasperV1)
            }
            _ => unreachable!(),
        }
    }
}

impl Display for TransactionTarget {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionTarget::Native => write!(formatter, "native"),
            TransactionTarget::Stored { id, runtime } => {
                write!(formatter, "stored({}, {})", id, runtime)
            }
            TransactionTarget::Session {
                module_bytes,
                runtime,
            } => write!(
                formatter,
                "session({} module bytes, {})",
                module_bytes.len(),
                runtime
            ),
        }
    }
}

impl Debug for TransactionTarget {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionTarget::Native => formatter.debug_struct("Native").finish(),
            TransactionTarget::Stored { id, runtime } => formatter
                .debug_struct("Stored")
                .field("id", id)
                .field("runtime", runtime)
                .finish(),
            TransactionTarget::Session {
                module_bytes,
                runtime,
            } => {
                struct BytesLen(usize);
                impl Debug for BytesLen {
                    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                        write!(formatter, "{} bytes", self.0)
                    }
                }

                formatter
                    .debug_struct("Session")
                    .field("module_bytes", &BytesLen(module_bytes.len()))
                    .field("runtime", runtime)
                    .finish()
            }
        }
    }
}

impl ToBytes for TransactionTarget {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            TransactionTarget::Native => serialize_native(),
            TransactionTarget::Stored { id, runtime } => serialize_stored(id, runtime),
            TransactionTarget::Session {
                module_bytes,
                runtime,
            } => serialize_session(module_bytes, runtime),
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            TransactionTarget::Native => native_serialized_length(),
            TransactionTarget::Stored { id, runtime } => stored_serialized_length(id, runtime),
            TransactionTarget::Session {
                module_bytes,
                runtime,
            } => session_serialized_length(module_bytes, runtime),
        }
    }
}

impl FromBytes for TransactionTarget {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        deserialize_transaction_target(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gens::transaction_target_arb;
    use proptest::prelude::*;
    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&TransactionTarget::random(rng));
        }
    }

    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in transaction_target_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
