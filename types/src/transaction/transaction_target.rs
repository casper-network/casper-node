use alloc::vec::Vec;
use core::fmt::{self, Debug, Display, Formatter};

use super::{
    serialization::CalltableSerializationEnvelope, TransactionInvocationTarget, TransactionRuntime,
};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{
        Bytes,
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    transaction::serialization::CalltableSerializationEnvelopeBuilder,
};
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

    fn serialized_field_lengths(&self) -> Vec<usize> {
        match self {
            TransactionTarget::Native => {
                vec![crate::bytesrepr::U8_SERIALIZED_LENGTH]
            }
            TransactionTarget::Stored { id, runtime } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    id.serialized_length(),
                    runtime.serialized_length(),
                ]
            }
            TransactionTarget::Session {
                module_bytes,
                runtime,
            } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    module_bytes.serialized_length(),
                    runtime.serialized_length(),
                ]
            }
        }
    }

    /// Returns a random `TransactionTarget`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => TransactionTarget::Native,
            1 => TransactionTarget::new_stored(
                TransactionInvocationTarget::random(rng),
                TransactionRuntime::VmCasperV1,
            ),
            2 => {
                let mut buffer = vec![0u8; rng.gen_range(0..100)];
                rng.fill_bytes(buffer.as_mut());
                TransactionTarget::new_session(Bytes::from(buffer), TransactionRuntime::VmCasperV1)
            }
            _ => unreachable!(),
        }
    }

    /// Returns `true` if the transaction target is [`Session`].
    ///
    /// [`Session`]: TransactionTarget::Session
    #[must_use]
    pub fn is_session(&self) -> bool {
        matches!(self, Self::Session { .. })
    }
}

const TAG_FIELD_INDEX: u16 = 0;

const NATIVE_VARIANT: u8 = 0;

const STORED_VARIANT: u8 = 1;
const STORED_ID_INDEX: u16 = 1;
const STORED_RUNTIME_INDEX: u16 = 2;

const SESSION_VARIANT: u8 = 2;
const SESSION_MODULE_BYTES_INDEX: u16 = 1;
const SESSION_RUNTIME_INDEX: u16 = 2;

impl ToBytes for TransactionTarget {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            TransactionTarget::Native => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &NATIVE_VARIANT)?
                    .binary_payload_bytes()
            }
            TransactionTarget::Stored { id, runtime } => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &STORED_VARIANT)?
                    .add_field(STORED_ID_INDEX, &id)?
                    .add_field(STORED_RUNTIME_INDEX, &runtime)?
                    .binary_payload_bytes()
            }
            TransactionTarget::Session {
                module_bytes,
                runtime,
            } => CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                .add_field(TAG_FIELD_INDEX, &SESSION_VARIANT)?
                .add_field(SESSION_MODULE_BYTES_INDEX, &module_bytes)?
                .add_field(SESSION_RUNTIME_INDEX, &runtime)?
                .binary_payload_bytes(),
        }
    }

    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for TransactionTarget {
    fn from_bytes(bytes: &[u8]) -> Result<(TransactionTarget, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(3, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;
        window.verify_index(TAG_FIELD_INDEX)?;
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            NATIVE_VARIANT => {
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionTarget::Native)
            }
            STORED_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(STORED_ID_INDEX)?;
                let (id, window) =
                    window.deserialize_and_maybe_next::<TransactionInvocationTarget>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(STORED_RUNTIME_INDEX)?;
                let (runtime, window) =
                    window.deserialize_and_maybe_next::<TransactionRuntime>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionTarget::Stored { id, runtime })
            }
            SESSION_VARIANT => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(SESSION_MODULE_BYTES_INDEX)?;
                let (module_bytes, window) = window.deserialize_and_maybe_next::<Bytes>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(SESSION_RUNTIME_INDEX)?;
                let (runtime, window) =
                    window.deserialize_and_maybe_next::<TransactionRuntime>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(TransactionTarget::Session {
                    module_bytes,
                    runtime,
                })
            }
            _ => Err(Formatting),
        };
        to_ret.map(|endpoint| (endpoint, remainder))
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

struct BytesLen(usize);
impl Debug for BytesLen {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} bytes", self.0)
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
            } => formatter
                .debug_struct("Session")
                .field("module_bytes", &BytesLen(module_bytes.len()))
                .field("runtime", runtime)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr, gens::transaction_target_arb};
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
