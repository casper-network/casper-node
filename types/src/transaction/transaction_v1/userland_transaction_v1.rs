use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{Rng, RngCore};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::DirectCallV1;
#[cfg(doc)]
use super::TransactionV1;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    addressable_entity::{
        DEFAULT_ENTRY_POINT_NAME, INSTALL_ENTRY_POINT_NAME, UPGRADE_ENTRY_POINT_NAME,
    },
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    PackageIdentifier, RuntimeArgs,
};

const STANDARD_TAG: u8 = 0;
const INSTALLER_UPGRADER_TAG: u8 = 1;
const DIRECT_CALL_TAG: u8 = 2;
const NOOP_TAG: u8 = 3;
const CLOSED_TAG: u8 = 4;

/// A [`TransactionV1`] with userland (i.e. not native) functionality.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A TransactionV1 with userland (i.e. not native) functionality.")
)]
#[serde(deny_unknown_fields)]
pub enum UserlandTransactionV1 {
    /// A general purpose transaction.
    Standard {
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// An installer or upgrader for a stored contract.
    InstallerUpgrader {
        /// If `Some`, this is an upgrade for the given contract, otherwise it is an installer.
        contract_package_id: Option<PackageIdentifier>,
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// A transaction targeting a stored contract.
    DirectCall(DirectCallV1),
    /// A transaction which doesn't modify global state.
    Noop {
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// A transaction which doesn't call stored contracts.
    Closed {
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
}

impl UserlandTransactionV1 {
    /// Returns a new `UserlandTransactionV1::Standard`.
    pub fn new_standard(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        UserlandTransactionV1::Standard { module_bytes, args }
    }

    /// Returns a new `UserlandTransactionV1::InstallerUpgrader` for installing.
    pub fn new_installer(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        UserlandTransactionV1::InstallerUpgrader {
            contract_package_id: None,
            module_bytes,
            args,
        }
    }

    /// Returns a new `UserlandTransactionV1::InstallerUpgrader` for upgrading.
    pub fn new_upgrader(
        contract_package_id: PackageIdentifier,
        module_bytes: Bytes,
        args: RuntimeArgs,
    ) -> Self {
        UserlandTransactionV1::InstallerUpgrader {
            contract_package_id: Some(contract_package_id),
            module_bytes,
            args,
        }
    }

    /// Returns a new `UserlandTransactionV1::Noop`.
    pub fn new_noop(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        UserlandTransactionV1::Noop { module_bytes, args }
    }

    /// Returns a new `UserlandTransactionV1::Closed`.
    pub fn new_closed(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        UserlandTransactionV1::Closed { module_bytes, args }
    }

    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            UserlandTransactionV1::Standard { args, .. }
            | UserlandTransactionV1::InstallerUpgrader { args, .. }
            | UserlandTransactionV1::Noop { args, .. }
            | UserlandTransactionV1::Closed { args, .. } => args,
            UserlandTransactionV1::DirectCall(direct_call) => direct_call.args(),
        }
    }

    pub(super) fn args_mut(&mut self) -> &mut RuntimeArgs {
        match self {
            UserlandTransactionV1::Standard { args, .. }
            | UserlandTransactionV1::InstallerUpgrader { args, .. }
            | UserlandTransactionV1::Noop { args, .. }
            | UserlandTransactionV1::Closed { args, .. } => args,
            UserlandTransactionV1::DirectCall(direct_call) => direct_call.args_mut(),
        }
    }

    /// Returns the entry point name.
    pub fn entry_point_name(&self) -> &str {
        match self {
            UserlandTransactionV1::Standard { .. }
            | UserlandTransactionV1::Noop { .. }
            | UserlandTransactionV1::Closed { .. } => DEFAULT_ENTRY_POINT_NAME,
            UserlandTransactionV1::InstallerUpgrader {
                contract_package_id: Some(_),
                ..
            } => UPGRADE_ENTRY_POINT_NAME,
            UserlandTransactionV1::InstallerUpgrader {
                contract_package_id: None,
                ..
            } => INSTALL_ENTRY_POINT_NAME,
            UserlandTransactionV1::DirectCall(direct_call) => direct_call.entry_point_name(),
        }
    }

    #[cfg(any(feature = "std", test))]
    pub(super) fn module_bytes_is_present_but_empty(&self) -> bool {
        match self {
            UserlandTransactionV1::Standard { module_bytes, .. }
            | UserlandTransactionV1::InstallerUpgrader { module_bytes, .. }
            | UserlandTransactionV1::Noop { module_bytes, .. }
            | UserlandTransactionV1::Closed { module_bytes, .. } => module_bytes.is_empty(),
            UserlandTransactionV1::DirectCall(_) => false,
        }
    }

    /// Returns a random `UserlandTransactionV1`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        fn random_bytes(rng: &mut TestRng) -> Bytes {
            let mut buffer = vec![0u8; rng.gen_range(0..100)];
            rng.fill_bytes(buffer.as_mut());
            Bytes::from(buffer)
        }

        match rng.gen_range(0..5) {
            0 => UserlandTransactionV1::Standard {
                module_bytes: random_bytes(rng),
                args: RuntimeArgs::random(rng),
            },
            1 => {
                let contract_package = rng.gen::<bool>().then(|| PackageIdentifier::random(rng));
                UserlandTransactionV1::InstallerUpgrader {
                    contract_package_id: contract_package,
                    module_bytes: random_bytes(rng),
                    args: RuntimeArgs::random(rng),
                }
            }
            2 => UserlandTransactionV1::DirectCall(DirectCallV1::random(rng)),
            3 => UserlandTransactionV1::Noop {
                module_bytes: random_bytes(rng),
                args: RuntimeArgs::random(rng),
            },
            4 => UserlandTransactionV1::Closed {
                module_bytes: random_bytes(rng),
                args: RuntimeArgs::random(rng),
            },
            _ => unreachable!(),
        }
    }
}

impl From<DirectCallV1> for UserlandTransactionV1 {
    fn from(direct_call: DirectCallV1) -> Self {
        UserlandTransactionV1::DirectCall(direct_call)
    }
}

impl Display for UserlandTransactionV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            UserlandTransactionV1::Standard { .. } => write!(formatter, "userland standard"),
            UserlandTransactionV1::InstallerUpgrader { .. } => {
                write!(formatter, "userland installer-upgrader")
            }
            UserlandTransactionV1::DirectCall(direct_call) => {
                write!(formatter, "userland {}", direct_call)
            }
            UserlandTransactionV1::Noop { .. } => write!(formatter, "userland noop"),
            UserlandTransactionV1::Closed { .. } => write!(formatter, "userland closed"),
        }
    }
}

impl ToBytes for UserlandTransactionV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            UserlandTransactionV1::Standard { module_bytes, args } => {
                STANDARD_TAG.write_bytes(writer)?;
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            UserlandTransactionV1::InstallerUpgrader {
                contract_package_id: contract_package,
                module_bytes,
                args,
            } => {
                INSTALLER_UPGRADER_TAG.write_bytes(writer)?;
                contract_package.write_bytes(writer)?;
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            UserlandTransactionV1::DirectCall(direct_call) => {
                DIRECT_CALL_TAG.write_bytes(writer)?;
                direct_call.write_bytes(writer)
            }
            UserlandTransactionV1::Noop { module_bytes, args } => {
                NOOP_TAG.write_bytes(writer)?;
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            UserlandTransactionV1::Closed { module_bytes, args } => {
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
                UserlandTransactionV1::Standard { module_bytes, args } => {
                    module_bytes.serialized_length() + args.serialized_length()
                }
                UserlandTransactionV1::InstallerUpgrader {
                    contract_package_id: contract_package,
                    module_bytes,
                    args,
                } => {
                    contract_package.serialized_length()
                        + module_bytes.serialized_length()
                        + args.serialized_length()
                }
                UserlandTransactionV1::DirectCall(direct_call) => direct_call.serialized_length(),
                UserlandTransactionV1::Noop { module_bytes, args } => {
                    module_bytes.serialized_length() + args.serialized_length()
                }
                UserlandTransactionV1::Closed { module_bytes, args } => {
                    module_bytes.serialized_length() + args.serialized_length()
                }
            }
    }
}

impl FromBytes for UserlandTransactionV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            STANDARD_TAG => {
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                let txn = UserlandTransactionV1::Standard { module_bytes, args };
                Ok((txn, remainder))
            }
            INSTALLER_UPGRADER_TAG => {
                let (contract_package, remainder) =
                    Option::<PackageIdentifier>::from_bytes(remainder)?;
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                let txn = UserlandTransactionV1::InstallerUpgrader {
                    contract_package_id: contract_package,
                    module_bytes,
                    args,
                };
                Ok((txn, remainder))
            }
            DIRECT_CALL_TAG => {
                let (direct_call, remainder) = DirectCallV1::from_bytes(remainder)?;
                let txn = UserlandTransactionV1::DirectCall(direct_call);
                Ok((txn, remainder))
            }
            NOOP_TAG => {
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                let txn = UserlandTransactionV1::Noop { module_bytes, args };
                Ok((txn, remainder))
            }
            CLOSED_TAG => {
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                let txn = UserlandTransactionV1::Closed { module_bytes, args };
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
            bytesrepr::test_serialization_roundtrip(&UserlandTransactionV1::random(rng));
        }
    }
}
