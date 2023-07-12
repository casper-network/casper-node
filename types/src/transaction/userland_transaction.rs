use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{Rng, RngCore};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::DirectCall;
#[cfg(doc)]
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    ContractPackageIdentifier, RuntimeArgs,
};

const STANDARD_TAG: u8 = 0;
const INSTALLER_UPGRADER_TAG: u8 = 1;
const DIRECT_CALL_TAG: u8 = 2;
const NOOP_TAG: u8 = 3;
const CLOSED_TAG: u8 = 4;

/// A [`Transaction`] with userland (i.e. not native) functionality.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A Transaction with userland (i.e. not native) functionality.")
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum UserlandTransaction {
    /// A general purpose `Transaction`.
    Standard {
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// An installer or upgrader for a stored contract.
    InstallerUpgrader {
        /// If `Some`, this is an upgrade for the given contract, otherwise it is an installer.
        contract_package_id: Option<ContractPackageIdentifier>,
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// A `Transaction` targeting a stored contract.
    DirectCall(DirectCall),
    /// A `Transaction` which doesn't modify global state.
    Noop {
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// A `Transaction` which doesn't call stored contracts.
    Closed {
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
}

impl UserlandTransaction {
    /// Returns a new `UserlandTransaction::Standard`.
    pub fn new_standard(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        UserlandTransaction::Standard { module_bytes, args }
    }

    /// Returns a new `UserlandTransaction::InstallerUpgrader` for installing.
    pub fn new_installer(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        UserlandTransaction::InstallerUpgrader {
            contract_package_id: None,
            module_bytes,
            args,
        }
    }

    /// Returns a new `UserlandTransaction::InstallerUpgrader` for upgrading.
    pub fn new_upgrader(
        contract_package_id: ContractPackageIdentifier,
        module_bytes: Bytes,
        args: RuntimeArgs,
    ) -> Self {
        UserlandTransaction::InstallerUpgrader {
            contract_package_id: Some(contract_package_id),
            module_bytes,
            args,
        }
    }

    /// Returns a new `UserlandTransaction::Noop`.
    pub fn new_noop(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        UserlandTransaction::Noop { module_bytes, args }
    }

    /// Returns a new `UserlandTransaction::Closed`.
    pub fn new_closed(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        UserlandTransaction::Closed { module_bytes, args }
    }

    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            UserlandTransaction::Standard { args, .. }
            | UserlandTransaction::InstallerUpgrader { args, .. }
            | UserlandTransaction::Noop { args, .. }
            | UserlandTransaction::Closed { args, .. } => args,
            UserlandTransaction::DirectCall(direct_call) => direct_call.args(),
        }
    }

    /// Returns a random `UserlandTransaction`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        fn random_bytes(rng: &mut TestRng) -> Bytes {
            let mut buffer = vec![0u8; rng.gen_range(0..100)];
            rng.fill_bytes(buffer.as_mut());
            Bytes::from(buffer)
        }

        match rng.gen_range(0..5) {
            0 => UserlandTransaction::Standard {
                module_bytes: random_bytes(rng),
                args: RuntimeArgs::random(rng),
            },
            1 => {
                let contract_package = rng
                    .gen::<bool>()
                    .then(|| ContractPackageIdentifier::random(rng));
                UserlandTransaction::InstallerUpgrader {
                    contract_package_id: contract_package,
                    module_bytes: random_bytes(rng),
                    args: RuntimeArgs::random(rng),
                }
            }
            2 => UserlandTransaction::DirectCall(DirectCall::random(rng)),
            3 => UserlandTransaction::Noop {
                module_bytes: random_bytes(rng),
                args: RuntimeArgs::random(rng),
            },
            4 => UserlandTransaction::Closed {
                module_bytes: random_bytes(rng),
                args: RuntimeArgs::random(rng),
            },
            _ => unreachable!(),
        }
    }
}

impl From<DirectCall> for UserlandTransaction {
    fn from(direct_call: DirectCall) -> Self {
        UserlandTransaction::DirectCall(direct_call)
    }
}

impl Display for UserlandTransaction {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            UserlandTransaction::Standard { .. } => write!(formatter, "userland standard"),
            UserlandTransaction::InstallerUpgrader { .. } => {
                write!(formatter, "userland installer-upgrader")
            }
            UserlandTransaction::DirectCall(direct_call) => {
                write!(formatter, "userland {}", direct_call)
            }
            UserlandTransaction::Noop { .. } => write!(formatter, "userland noop"),
            UserlandTransaction::Closed { .. } => write!(formatter, "userland closed"),
        }
    }
}

impl ToBytes for UserlandTransaction {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            UserlandTransaction::Standard { module_bytes, args } => {
                STANDARD_TAG.write_bytes(writer)?;
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            UserlandTransaction::InstallerUpgrader {
                contract_package_id: contract_package,
                module_bytes,
                args,
            } => {
                INSTALLER_UPGRADER_TAG.write_bytes(writer)?;
                contract_package.write_bytes(writer)?;
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            UserlandTransaction::DirectCall(direct_call) => {
                DIRECT_CALL_TAG.write_bytes(writer)?;
                direct_call.write_bytes(writer)
            }
            UserlandTransaction::Noop { module_bytes, args } => {
                NOOP_TAG.write_bytes(writer)?;
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            UserlandTransaction::Closed { module_bytes, args } => {
                CLOSED_TAG.write_bytes(writer)?;
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
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
                UserlandTransaction::Standard { module_bytes, args } => {
                    module_bytes.serialized_length() + args.serialized_length()
                }
                UserlandTransaction::InstallerUpgrader {
                    contract_package_id: contract_package,
                    module_bytes,
                    args,
                } => {
                    contract_package.serialized_length()
                        + module_bytes.serialized_length()
                        + args.serialized_length()
                }
                UserlandTransaction::DirectCall(direct_call) => direct_call.serialized_length(),
                UserlandTransaction::Noop { module_bytes, args } => {
                    module_bytes.serialized_length() + args.serialized_length()
                }
                UserlandTransaction::Closed { module_bytes, args } => {
                    module_bytes.serialized_length() + args.serialized_length()
                }
            }
    }
}

impl FromBytes for UserlandTransaction {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            STANDARD_TAG => {
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                let txn = UserlandTransaction::Standard { module_bytes, args };
                Ok((txn, remainder))
            }
            INSTALLER_UPGRADER_TAG => {
                let (contract_package, remainder) =
                    Option::<ContractPackageIdentifier>::from_bytes(remainder)?;
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                let txn = UserlandTransaction::InstallerUpgrader {
                    contract_package_id: contract_package,
                    module_bytes,
                    args,
                };
                Ok((txn, remainder))
            }
            DIRECT_CALL_TAG => {
                let (direct_call, remainder) = DirectCall::from_bytes(remainder)?;
                let txn = UserlandTransaction::DirectCall(direct_call);
                Ok((txn, remainder))
            }
            NOOP_TAG => {
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                let txn = UserlandTransaction::Noop { module_bytes, args };
                Ok((txn, remainder))
            }
            CLOSED_TAG => {
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                let txn = UserlandTransaction::Closed { module_bytes, args };
                Ok((txn, remainder))
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
            bytesrepr::test_serialization_roundtrip(&UserlandTransaction::random(rng));
        }
    }
}
