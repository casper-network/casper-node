use alloc::vec::Vec;
use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{Rng, RngCore};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
use super::{TransactionInvocationTarget, TransactionRuntime, TransactionSessionKind};
use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

const NATIVE_TAG: u8 = 0;
const STORED_TAG: u8 = 1;
const SESSION_TAG: u8 = 2;

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
        /// The kind of session.
        kind: TransactionSessionKind,
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
    pub fn new_session(
        kind: TransactionSessionKind,
        module_bytes: Bytes,
        runtime: TransactionRuntime,
    ) -> Self {
        TransactionTarget::Session {
            kind,
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
                TransactionTarget::new_session(
                    TransactionSessionKind::random(rng),
                    Bytes::from(buffer),
                    TransactionRuntime::VmCasperV1,
                )
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
                kind,
                module_bytes,
                runtime,
            } => write!(
                formatter,
                "session({}, {} module bytes, {})",
                kind,
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
                kind,
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
                    .field("kind", kind)
                    .field("module_bytes", &BytesLen(module_bytes.len()))
                    .field("runtime", runtime)
                    .finish()
            }
        }
    }
}

impl ToBytes for TransactionTarget {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionTarget::Native => NATIVE_TAG.write_bytes(writer),
            TransactionTarget::Stored { id, runtime } => {
                STORED_TAG.write_bytes(writer)?;
                id.write_bytes(writer)?;
                runtime.write_bytes(writer)
            }
            TransactionTarget::Session {
                kind,
                module_bytes,
                runtime,
            } => {
                SESSION_TAG.write_bytes(writer)?;
                kind.write_bytes(writer)?;
                module_bytes.write_bytes(writer)?;
                runtime.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TransactionTarget::Native => 0,
                TransactionTarget::Stored { id, runtime } => {
                    id.serialized_length() + runtime.serialized_length()
                }
                TransactionTarget::Session {
                    kind,
                    module_bytes,
                    runtime,
                } => {
                    kind.serialized_length()
                        + module_bytes.serialized_length()
                        + runtime.serialized_length()
                }
            }
    }
}

impl FromBytes for TransactionTarget {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            NATIVE_TAG => Ok((TransactionTarget::Native, remainder)),
            STORED_TAG => {
                let (id, remainder) = TransactionInvocationTarget::from_bytes(remainder)?;
                let (runtime, remainder) = TransactionRuntime::from_bytes(remainder)?;
                let target = TransactionTarget::new_stored(id, runtime);
                Ok((target, remainder))
            }
            SESSION_TAG => {
                let (kind, remainder) = TransactionSessionKind::from_bytes(remainder)?;
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (runtime, remainder) = TransactionRuntime::from_bytes(remainder)?;
                let target = TransactionTarget::new_session(kind, module_bytes, runtime);
                Ok((target, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&TransactionTarget::random(rng));
        }
    }
}
