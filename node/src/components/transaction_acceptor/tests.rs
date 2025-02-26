#![cfg(test)]

use std::{
    collections::{BTreeMap, VecDeque},
    fmt::{self, Debug, Display, Formatter},
    iter,
    sync::Arc,
    time::Duration,
};

use derive_more::From;
use futures::{
    channel::oneshot::{self, Sender},
    FutureExt,
};
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;
use tokio::time;

use casper_storage::{
    data_access_layer::{
        AddressableEntityResult, BalanceIdentifier, BalanceResult, EntryPointExistsResult,
        ProofsResult, QueryResult,
    },
    tracking_copy::TrackingCopyError,
};
use casper_types::{
    account::{Account, AccountHash, ActionThresholds, AssociatedKeys, Weight},
    addressable_entity::AddressableEntity,
    bytesrepr::Bytes,
    contracts::{ContractPackage, NamedKeys},
    global_state::TrieMerkleProof,
    testing::TestRng,
    Block, BlockV2, CLValue, Chainspec, ChainspecRawBytes, Contract, Deploy, EraId, HashAddr,
    InvalidDeploy, InvalidTransaction, InvalidTransactionV1, Key, PricingHandling, PricingMode,
    ProtocolVersion, PublicKey, SecretKey, StoredValue, TestBlockBuilder, TimeDiff, Timestamp,
    Transaction, TransactionArgs, TransactionConfig, TransactionRuntimeParams, TransactionV1, URef,
    DEFAULT_BASELINE_MOTES_AMOUNT,
};

use super::*;
use crate::{
    components::{
        network::Identity as NetworkIdentity,
        storage::{self, Storage},
    },
    effect::{
        announcements::{ControlAnnouncement, TransactionAcceptorAnnouncement},
        requests::{
            ContractRuntimeRequest, MakeBlockExecutableRequest, MarkBlockCompletedRequest,
            NetworkRequest,
        },
        Responder,
    },
    logging,
    protocol::Message,
    reactor::{self, EventQueueHandle, QueueKind, Runner, TryCrankOutcome},
    testing::ConditionCheckReactor,
    types::{transaction::transaction_v1_builder::TransactionV1Builder, NodeId},
    utils::{Loadable, WithDir},
    NodeRng,
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const TIMEOUT: Duration = Duration::from_secs(30);

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[allow(clippy::large_enum_variant)]
#[must_use]
enum Event {
    #[from]
    Storage(#[serde(skip_serializing)] storage::Event),
    #[from]
    TransactionAcceptor(#[serde(skip_serializing)] super::Event),
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    FatalAnnouncement(FatalAnnouncement),
    #[from]
    TransactionAcceptorAnnouncement(#[serde(skip_serializing)] TransactionAcceptorAnnouncement),
    #[from]
    ContractRuntime(#[serde(skip_serializing)] ContractRuntimeRequest),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    NetworkRequest(NetworkRequest<Message>),
}

impl From<MakeBlockExecutableRequest> for Event {
    fn from(request: MakeBlockExecutableRequest) -> Self {
        Event::Storage(storage::Event::MakeBlockExecutableRequest(Box::new(
            request,
        )))
    }
}

impl From<MarkBlockCompletedRequest> for Event {
    fn from(request: MarkBlockCompletedRequest) -> Self {
        Event::Storage(storage::Event::MarkBlockCompletedRequest(request))
    }
}

impl From<ControlAnnouncement> for Event {
    fn from(control_announcement: ControlAnnouncement) -> Self {
        Event::ControlAnnouncement(control_announcement)
    }
}

impl ReactorEvent for Event {
    fn is_control(&self) -> bool {
        matches!(self, Event::ControlAnnouncement(_))
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::TransactionAcceptor(event) => {
                write!(formatter, "transaction acceptor: {}", event)
            }
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::FatalAnnouncement(fatal_ann) => write!(formatter, "fatal: {}", fatal_ann),
            Event::TransactionAcceptorAnnouncement(ann) => {
                write!(formatter, "transaction-acceptor announcement: {}", ann)
            }

            Event::ContractRuntime(event) => {
                write!(formatter, "contract-runtime event: {:?}", event)
            }
            Event::StorageRequest(request) => write!(formatter, "storage request: {:?}", request),
            Event::NetworkRequest(request) => write!(formatter, "network request: {:?}", request),
        }
    }
}

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
enum Error {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ContractScenario {
    Valid,
    MissingContractAtHash,
    MissingContractAtName,
    MissingEntryPoint,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ContractPackageScenario {
    Valid,
    MissingPackageAtHash,
    MissingPackageAtName,
    MissingContractVersion,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TxnType {
    Deploy,
    V1,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TestScenario {
    FromPeerInvalidTransaction(TxnType),
    FromPeerExpired(TxnType),
    FromPeerValidTransaction(TxnType),
    FromPeerRepeatedValidTransaction(TxnType),
    FromPeerMissingAccount(TxnType),
    FromPeerAccountWithInsufficientWeight(TxnType),
    FromPeerAccountWithInvalidAssociatedKeys(TxnType),
    FromPeerCustomPaymentContract(ContractScenario),
    FromPeerCustomPaymentContractPackage(ContractPackageScenario),
    FromPeerSessionContract(TxnType, ContractScenario),
    FromPeerSessionContractPackage(TxnType, ContractPackageScenario),
    FromClientInvalidTransaction(TxnType),
    FromClientSlightlyFutureDatedTransaction(TxnType),
    FromClientFutureDatedTransaction(TxnType),
    FromClientExpired(TxnType),
    FromClientMissingAccount(TxnType),
    FromClientInsufficientBalance(TxnType),
    FromClientValidTransaction(TxnType),
    FromClientRepeatedValidTransaction(TxnType),
    FromClientAccountWithInsufficientWeight(TxnType),
    FromClientAccountWithInvalidAssociatedKeys(TxnType),
    AccountWithUnknownBalance,
    FromClientCustomPaymentContract(ContractScenario),
    FromClientCustomPaymentContractPackage(ContractPackageScenario),
    FromClientSessionContract(TxnType, ContractScenario),
    FromClientSessionContractPackage(TxnType, ContractPackageScenario),
    FromClientSignedByAdmin(TxnType),
    DeployWithNativeTransferInPayment,
    DeployWithEmptySessionModuleBytes,
    DeployWithoutPaymentAmount,
    DeployWithMangledPaymentAmount,
    DeployWithMangledTransferAmount,
    DeployWithoutTransferTarget,
    DeployWithoutTransferAmount,
    BalanceCheckForDeploySentByPeer,
    InvalidPricingModeForTransactionV1,
    TooLowGasPriceToleranceForTransactionV1,
    TooLowGasPriceToleranceForDeploy,
    InvalidFields,
    InvalidFieldsFromPeer,
    InvalidArgumentsKind,
    WasmTransactionWithTooBigPayment,
    WasmDeployWithTooBigPayment,
}

impl TestScenario {
    fn source(&self, rng: &mut NodeRng) -> Source {
        match self {
            TestScenario::FromPeerInvalidTransaction(_)
            | TestScenario::FromPeerExpired(_)
            | TestScenario::FromPeerValidTransaction(_)
            | TestScenario::FromPeerRepeatedValidTransaction(_)
            | TestScenario::BalanceCheckForDeploySentByPeer
            | TestScenario::FromPeerMissingAccount(_)
            | TestScenario::FromPeerAccountWithInsufficientWeight(_)
            | TestScenario::FromPeerAccountWithInvalidAssociatedKeys(_)
            | TestScenario::FromPeerCustomPaymentContract(_)
            | TestScenario::FromPeerCustomPaymentContractPackage(_)
            | TestScenario::FromPeerSessionContract(..)
            | TestScenario::FromPeerSessionContractPackage(..)
            | TestScenario::InvalidFieldsFromPeer => Source::Peer(NodeId::random(rng)),
            TestScenario::FromClientInvalidTransaction(_)
            | TestScenario::FromClientSlightlyFutureDatedTransaction(_)
            | TestScenario::FromClientFutureDatedTransaction(_)
            | TestScenario::FromClientExpired(_)
            | TestScenario::FromClientMissingAccount(_)
            | TestScenario::FromClientInsufficientBalance(_)
            | TestScenario::FromClientValidTransaction(_)
            | TestScenario::FromClientRepeatedValidTransaction(_)
            | TestScenario::FromClientAccountWithInsufficientWeight(_)
            | TestScenario::FromClientAccountWithInvalidAssociatedKeys(_)
            | TestScenario::AccountWithUnknownBalance
            | TestScenario::DeployWithoutPaymentAmount
            | TestScenario::DeployWithMangledPaymentAmount
            | TestScenario::DeployWithMangledTransferAmount
            | TestScenario::DeployWithoutTransferAmount
            | TestScenario::DeployWithoutTransferTarget
            | TestScenario::FromClientCustomPaymentContract(_)
            | TestScenario::FromClientCustomPaymentContractPackage(_)
            | TestScenario::FromClientSessionContract(..)
            | TestScenario::FromClientSessionContractPackage(..)
            | TestScenario::FromClientSignedByAdmin(_)
            | TestScenario::DeployWithEmptySessionModuleBytes
            | TestScenario::DeployWithNativeTransferInPayment
            | TestScenario::InvalidPricingModeForTransactionV1
            | TestScenario::TooLowGasPriceToleranceForTransactionV1
            | TestScenario::TooLowGasPriceToleranceForDeploy
            | TestScenario::InvalidFields
            | TestScenario::InvalidArgumentsKind
            | TestScenario::WasmTransactionWithTooBigPayment
            | TestScenario::WasmDeployWithTooBigPayment => Source::Client,
        }
    }

    fn transaction(&self, rng: &mut TestRng, admin: &SecretKey) -> Transaction {
        let secret_key = SecretKey::random(rng);
        match self {
            TestScenario::FromPeerInvalidTransaction(TxnType::Deploy)
            | TestScenario::FromClientInvalidTransaction(TxnType::Deploy) => {
                let mut deploy = Deploy::random_valid_native_transfer(rng);
                deploy.invalidate();
                Transaction::from(deploy)
            }
            TestScenario::FromPeerInvalidTransaction(TxnType::V1)
            | TestScenario::FromClientInvalidTransaction(TxnType::V1) => {
                let mut txn = TransactionV1::random(rng);
                txn.invalidate();
                Transaction::from(txn)
            }
            TestScenario::FromPeerExpired(TxnType::Deploy)
            | TestScenario::FromClientExpired(TxnType::Deploy) => {
                Transaction::from(Deploy::random_expired_deploy(rng))
            }
            TestScenario::FromPeerExpired(TxnType::V1)
            | TestScenario::FromClientExpired(TxnType::V1) => {
                let txn = TransactionV1Builder::new_session(
                    false,
                    Bytes::from(vec![1]),
                    TransactionRuntimeParams::VmCasperV1,
                )
                .with_chain_name("casper-example")
                .with_timestamp(Timestamp::zero())
                .with_secret_key(&secret_key)
                .build()
                .unwrap();
                Transaction::from(txn)
            }
            TestScenario::FromPeerValidTransaction(txn_type)
            | TestScenario::FromPeerRepeatedValidTransaction(txn_type)
            | TestScenario::FromPeerMissingAccount(txn_type)
            | TestScenario::FromPeerAccountWithInvalidAssociatedKeys(txn_type)
            | TestScenario::FromPeerAccountWithInsufficientWeight(txn_type)
            | TestScenario::FromClientMissingAccount(txn_type)
            | TestScenario::FromClientInsufficientBalance(txn_type)
            | TestScenario::FromClientValidTransaction(txn_type)
            | TestScenario::FromClientRepeatedValidTransaction(txn_type)
            | TestScenario::FromClientAccountWithInvalidAssociatedKeys(txn_type)
            | TestScenario::FromClientAccountWithInsufficientWeight(txn_type) => match txn_type {
                TxnType::Deploy => Transaction::from(Deploy::random_valid_native_transfer(rng)),
                TxnType::V1 => {
                    let txn = TransactionV1Builder::new_session(
                        false,
                        Bytes::from(vec![1]),
                        TransactionRuntimeParams::VmCasperV1,
                    )
                    .with_chain_name("casper-example")
                    .with_timestamp(Timestamp::now())
                    .with_secret_key(&secret_key)
                    .build()
                    .unwrap();
                    Transaction::from(txn)
                }
            },
            TestScenario::FromClientSignedByAdmin(TxnType::Deploy) => {
                let mut deploy = Deploy::random_valid_native_transfer(rng);
                deploy.sign(admin);
                Transaction::from(deploy)
            }
            TestScenario::FromClientSignedByAdmin(TxnType::V1) => {
                let txn = TransactionV1Builder::new_session(
                    false,
                    Bytes::from(vec![1]),
                    TransactionRuntimeParams::VmCasperV1,
                )
                .with_chain_name("casper-example")
                .with_timestamp(Timestamp::now())
                .with_secret_key(admin)
                .build()
                .unwrap();
                Transaction::from(txn)
            }
            TestScenario::AccountWithUnknownBalance
            | TestScenario::BalanceCheckForDeploySentByPeer => {
                Transaction::from(Deploy::random_valid_native_transfer(rng))
            }
            TestScenario::DeployWithoutPaymentAmount => {
                Transaction::from(Deploy::random_without_payment_amount(rng))
            }
            TestScenario::DeployWithMangledPaymentAmount => {
                Transaction::from(Deploy::random_with_mangled_payment_amount(rng))
            }
            TestScenario::DeployWithoutTransferTarget => {
                Transaction::from(Deploy::random_without_transfer_target(rng))
            }
            TestScenario::DeployWithoutTransferAmount => {
                Transaction::from(Deploy::random_without_transfer_amount(rng))
            }
            TestScenario::DeployWithMangledTransferAmount => {
                Transaction::from(Deploy::random_with_mangled_transfer_amount(rng))
            }

            TestScenario::FromPeerCustomPaymentContract(contract_scenario)
            | TestScenario::FromClientCustomPaymentContract(contract_scenario) => {
                match contract_scenario {
                    ContractScenario::Valid | ContractScenario::MissingContractAtName => {
                        Transaction::from(
                            Deploy::random_with_valid_custom_payment_contract_by_name(rng),
                        )
                    }
                    ContractScenario::MissingEntryPoint => Transaction::from(
                        Deploy::random_with_missing_entry_point_in_payment_contract(rng),
                    ),
                    ContractScenario::MissingContractAtHash => {
                        Transaction::from(Deploy::random_with_missing_payment_contract_by_hash(rng))
                    }
                }
            }
            TestScenario::FromPeerCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromClientCustomPaymentContractPackage(contract_package_scenario) => {
                match contract_package_scenario {
                    ContractPackageScenario::Valid
                    | ContractPackageScenario::MissingPackageAtName => Transaction::from(
                        Deploy::random_with_valid_custom_payment_package_by_name(rng),
                    ),
                    ContractPackageScenario::MissingPackageAtHash => {
                        Transaction::from(Deploy::random_with_missing_payment_package_by_hash(rng))
                    }
                    ContractPackageScenario::MissingContractVersion => Transaction::from(
                        Deploy::random_with_nonexistent_contract_version_in_payment_package(rng),
                    ),
                }
            }
            TestScenario::FromPeerSessionContract(TxnType::Deploy, contract_scenario)
            | TestScenario::FromClientSessionContract(TxnType::Deploy, contract_scenario) => {
                match contract_scenario {
                    ContractScenario::Valid | ContractScenario::MissingContractAtName => {
                        Transaction::from(Deploy::random_with_valid_session_contract_by_name(rng))
                    }
                    ContractScenario::MissingContractAtHash => {
                        Transaction::from(Deploy::random_with_missing_session_contract_by_hash(rng))
                    }
                    ContractScenario::MissingEntryPoint => Transaction::from(
                        Deploy::random_with_missing_entry_point_in_session_contract(rng),
                    ),
                }
            }
            TestScenario::FromPeerSessionContract(TxnType::V1, contract_scenario)
            | TestScenario::FromClientSessionContract(TxnType::V1, contract_scenario) => {
                match contract_scenario {
                    ContractScenario::Valid | ContractScenario::MissingContractAtName => {
                        let txn = TransactionV1Builder::new_targeting_invocable_entity_via_alias(
                            "Test",
                            "call",
                            TransactionRuntimeParams::VmCasperV1,
                        )
                        .with_chain_name("casper-example")
                        .with_timestamp(Timestamp::now())
                        .with_secret_key(&secret_key)
                        .build()
                        .unwrap();
                        Transaction::from(txn)
                    }
                    ContractScenario::MissingContractAtHash => {
                        let txn = TransactionV1Builder::new_targeting_invocable_entity(
                            AddressableEntityHash::new(HashAddr::default()),
                            "call",
                            TransactionRuntimeParams::VmCasperV1,
                        )
                        .with_chain_name("casper-example")
                        .with_timestamp(Timestamp::now())
                        .with_secret_key(&secret_key)
                        .build()
                        .unwrap();
                        Transaction::from(txn)
                    }
                    ContractScenario::MissingEntryPoint => {
                        let txn = TransactionV1Builder::new_targeting_invocable_entity(
                            AddressableEntityHash::new(HashAddr::default()),
                            "non-existent-entry-point",
                            TransactionRuntimeParams::VmCasperV1,
                        )
                        .with_chain_name("casper-example")
                        .with_timestamp(Timestamp::now())
                        .with_secret_key(&secret_key)
                        .build()
                        .unwrap();
                        Transaction::from(txn)
                    }
                }
            }
            TestScenario::FromPeerSessionContractPackage(
                TxnType::Deploy,
                contract_package_scenario,
            )
            | TestScenario::FromClientSessionContractPackage(
                TxnType::Deploy,
                contract_package_scenario,
            ) => match contract_package_scenario {
                ContractPackageScenario::Valid | ContractPackageScenario::MissingPackageAtName => {
                    Transaction::from(Deploy::random_with_valid_session_package_by_name(rng))
                }
                ContractPackageScenario::MissingPackageAtHash => {
                    Transaction::from(Deploy::random_with_missing_session_package_by_hash(rng))
                }
                ContractPackageScenario::MissingContractVersion => Transaction::from(
                    Deploy::random_with_nonexistent_contract_version_in_session_package(rng),
                ),
            },
            TestScenario::FromPeerSessionContractPackage(
                TxnType::V1,
                contract_package_scenario,
            )
            | TestScenario::FromClientSessionContractPackage(
                TxnType::V1,
                contract_package_scenario,
            ) => match contract_package_scenario {
                ContractPackageScenario::Valid | ContractPackageScenario::MissingPackageAtName => {
                    let txn = TransactionV1Builder::new_targeting_package_via_alias(
                        "Test",
                        None,
                        "call",
                        TransactionRuntimeParams::VmCasperV1,
                    )
                    .with_chain_name("casper-example")
                    .with_timestamp(Timestamp::now())
                    .with_secret_key(&secret_key)
                    .build()
                    .unwrap();
                    Transaction::from(txn)
                }
                ContractPackageScenario::MissingPackageAtHash => {
                    let txn = TransactionV1Builder::new_targeting_package(
                        PackageHash::new(PackageAddr::default()),
                        None,
                        "call",
                        TransactionRuntimeParams::VmCasperV1,
                    )
                    .with_chain_name("casper-example")
                    .with_timestamp(Timestamp::now())
                    .with_secret_key(&secret_key)
                    .build()
                    .unwrap();
                    Transaction::from(txn)
                }
                ContractPackageScenario::MissingContractVersion => {
                    let txn = TransactionV1Builder::new_targeting_package(
                        PackageHash::new(PackageAddr::default()),
                        Some(6),
                        "call",
                        TransactionRuntimeParams::VmCasperV1,
                    )
                    .with_chain_name("casper-example")
                    .with_timestamp(Timestamp::now())
                    .with_secret_key(&secret_key)
                    .build()
                    .unwrap();
                    Transaction::from(txn)
                }
            },
            TestScenario::DeployWithEmptySessionModuleBytes => {
                Transaction::from(Deploy::random_with_empty_session_module_bytes(rng))
            }
            TestScenario::DeployWithNativeTransferInPayment => {
                Transaction::from(Deploy::random_with_native_transfer_in_payment_logic(rng))
            }
            TestScenario::FromClientSlightlyFutureDatedTransaction(txn_type) => {
                let timestamp = Timestamp::now() + (Config::default().timestamp_leeway / 2);
                let ttl = TimeDiff::from_seconds(300);
                match txn_type {
                    TxnType::Deploy => Transaction::from(
                        Deploy::random_valid_native_transfer_with_timestamp_and_ttl(
                            rng, timestamp, ttl,
                        ),
                    ),
                    TxnType::V1 => {
                        let txn = TransactionV1Builder::new_session(
                            false,
                            Bytes::from(vec![1]),
                            TransactionRuntimeParams::VmCasperV1,
                        )
                        .with_chain_name("casper-example")
                        .with_timestamp(timestamp)
                        .with_ttl(ttl)
                        .with_secret_key(&secret_key)
                        .build()
                        .unwrap();
                        Transaction::from(txn)
                    }
                }
            }
            TestScenario::FromClientFutureDatedTransaction(txn_type) => {
                let timestamp = Timestamp::now()
                    + Config::default().timestamp_leeway
                    + TimeDiff::from_millis(100);
                let ttl = TimeDiff::from_seconds(300);
                match txn_type {
                    TxnType::Deploy => Transaction::from(
                        Deploy::random_valid_native_transfer_with_timestamp_and_ttl(
                            rng, timestamp, ttl,
                        ),
                    ),
                    TxnType::V1 => {
                        let txn = TransactionV1Builder::new_session(
                            false,
                            Bytes::from(vec![1]),
                            TransactionRuntimeParams::VmCasperV1,
                        )
                        .with_chain_name("casper-example")
                        .with_timestamp(timestamp)
                        .with_ttl(ttl)
                        .with_secret_key(&secret_key)
                        .build()
                        .unwrap();
                        Transaction::from(txn)
                    }
                }
            }
            TestScenario::InvalidPricingModeForTransactionV1 => {
                let classic_mode_transaction = TransactionV1Builder::new_random(rng)
                    .with_pricing_mode(PricingMode::Fixed {
                        gas_price_tolerance: 5,
                        additional_computation_factor: 0,
                    })
                    .with_chain_name("casper-example")
                    .build()
                    .expect("must create classic mode transaction");
                Transaction::from(classic_mode_transaction)
            }
            TestScenario::TooLowGasPriceToleranceForTransactionV1 => {
                const TOO_LOW_GAS_PRICE_TOLERANCE: u8 = 0;

                let fixed_mode_transaction = TransactionV1Builder::new_random(rng)
                    .with_pricing_mode(PricingMode::Fixed {
                        gas_price_tolerance: TOO_LOW_GAS_PRICE_TOLERANCE,
                        additional_computation_factor: 0,
                    })
                    .with_chain_name("casper-example")
                    .build()
                    .expect("must create fixed mode transaction");
                Transaction::from(fixed_mode_transaction)
            }
            TestScenario::TooLowGasPriceToleranceForDeploy => {
                const TOO_LOW_GAS_PRICE_TOLERANCE: u64 = 0;

                let deploy = Deploy::random_with_gas_price(rng, TOO_LOW_GAS_PRICE_TOLERANCE);
                Transaction::from(deploy)
            }
            TestScenario::InvalidFields | TestScenario::InvalidFieldsFromPeer => {
                let mut additional_fields = BTreeMap::new();
                additional_fields.insert(42, Bytes::from(vec![1]));
                let txn = TransactionV1Builder::new_session(
                    false,
                    Bytes::from(vec![1]),
                    TransactionRuntimeParams::VmCasperV1,
                )
                .with_chain_name("casper-example")
                .with_ttl(TimeDiff::from_seconds(300))
                .with_secret_key(&secret_key)
                .with_additional_fields(additional_fields)
                .build()
                .unwrap();
                Transaction::from(txn)
            }
            TestScenario::InvalidArgumentsKind => {
                let timestamp = Timestamp::now()
                    + Config::default().timestamp_leeway
                    + TimeDiff::from_millis(100);
                let ttl = TimeDiff::from_seconds(300);
                let txn = TransactionV1Builder::new_session(
                    false,
                    Bytes::from(vec![1]),
                    TransactionRuntimeParams::VmCasperV1,
                )
                .with_transaction_args(TransactionArgs::Bytesrepr(Bytes::from(vec![1, 2, 3])))
                .with_chain_name("casper-example")
                .with_timestamp(timestamp)
                .with_ttl(ttl)
                .with_secret_key(&secret_key)
                .build()
                .unwrap();
                Transaction::from(txn)
            }
            TestScenario::WasmTransactionWithTooBigPayment => {
                let timestamp = Timestamp::now()
                    + Config::default().timestamp_leeway
                    + TimeDiff::from_millis(100);
                let ttl = TimeDiff::from_seconds(300);
                let txn = TransactionV1Builder::new_session(
                    false,
                    Bytes::from(vec![1]),
                    TransactionRuntimeParams::VmCasperV1,
                )
                .with_pricing_mode(PricingMode::PaymentLimited {
                    payment_amount: u64::MAX, /* make sure it's a big value that doesn't match
                                               * any wasm lane */
                    gas_price_tolerance: 2,
                    standard_payment: true,
                })
                .with_chain_name("casper-example")
                .with_timestamp(timestamp)
                .with_ttl(ttl)
                .with_secret_key(&secret_key)
                .build()
                .unwrap();
                Transaction::from(txn)
            }
            TestScenario::WasmDeployWithTooBigPayment => {
                Transaction::from(Deploy::random_with_oversized_payment_amount(rng))
            }
        }
    }

    fn is_valid_transaction_case(&self) -> bool {
        match self {
            TestScenario::FromPeerRepeatedValidTransaction(_)
            | TestScenario::FromPeerExpired(_)
            | TestScenario::FromPeerValidTransaction(_)
            | TestScenario::FromPeerMissingAccount(_) // account check skipped if from peer
            | TestScenario::FromPeerAccountWithInsufficientWeight(_) // account check skipped if from peer
            | TestScenario::FromPeerAccountWithInvalidAssociatedKeys(_) // account check skipped if from peer
            | TestScenario::FromClientRepeatedValidTransaction(_)
            | TestScenario::FromClientValidTransaction(_)
            | TestScenario::FromClientSlightlyFutureDatedTransaction(_)
            | TestScenario::FromClientSignedByAdmin(..) => true,
            TestScenario::FromPeerInvalidTransaction(_)
            | TestScenario::FromClientInsufficientBalance(_)
            | TestScenario::FromClientMissingAccount(_)
            | TestScenario::FromClientInvalidTransaction(_)
            | TestScenario::FromClientFutureDatedTransaction(_)
            | TestScenario::FromClientAccountWithInsufficientWeight(_)
            | TestScenario::FromClientAccountWithInvalidAssociatedKeys(_)
            | TestScenario::AccountWithUnknownBalance
            | TestScenario::DeployWithEmptySessionModuleBytes
            | TestScenario::DeployWithNativeTransferInPayment
            | TestScenario::DeployWithoutPaymentAmount
            | TestScenario::DeployWithMangledPaymentAmount
            | TestScenario::DeployWithMangledTransferAmount
            | TestScenario::DeployWithoutTransferAmount
            | TestScenario::DeployWithoutTransferTarget
            | TestScenario::BalanceCheckForDeploySentByPeer
            | TestScenario::FromClientExpired(_) => false,
            TestScenario::FromPeerCustomPaymentContract(contract_scenario)
            | TestScenario::FromPeerSessionContract(_, contract_scenario)
            | TestScenario::FromClientCustomPaymentContract(contract_scenario)
            | TestScenario::FromClientSessionContract(_, contract_scenario) => match contract_scenario
            {
                ContractScenario::Valid
                | ContractScenario::MissingContractAtName => true,
                | ContractScenario::MissingContractAtHash
                | ContractScenario::MissingEntryPoint => false,
            },
            TestScenario::FromPeerCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromPeerSessionContractPackage(_, contract_package_scenario)
            | TestScenario::FromClientCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromClientSessionContractPackage(_, contract_package_scenario) => {
                match contract_package_scenario {
                    ContractPackageScenario::Valid
                    | ContractPackageScenario::MissingPackageAtName => true,
                    | ContractPackageScenario::MissingPackageAtHash
                    | ContractPackageScenario::MissingContractVersion => false,
                }
            }
            TestScenario::InvalidPricingModeForTransactionV1 => false,
            TestScenario::TooLowGasPriceToleranceForTransactionV1 => false,
            TestScenario::TooLowGasPriceToleranceForDeploy => false,
            TestScenario::InvalidFields => false,
            TestScenario::InvalidFieldsFromPeer => false,
            TestScenario::InvalidArgumentsKind => false,
            TestScenario::WasmTransactionWithTooBigPayment => false,
            TestScenario::WasmDeployWithTooBigPayment => false,
        }
    }

    fn is_repeated_transaction_case(&self) -> bool {
        matches!(
            self,
            TestScenario::FromClientRepeatedValidTransaction(_)
                | TestScenario::FromPeerRepeatedValidTransaction(_)
        )
    }

    fn contract_scenario(&self) -> Option<ContractScenario> {
        match self {
            TestScenario::FromPeerCustomPaymentContract(contract_scenario)
            | TestScenario::FromPeerSessionContract(_, contract_scenario)
            | TestScenario::FromClientCustomPaymentContract(contract_scenario)
            | TestScenario::FromClientSessionContract(_, contract_scenario) => {
                Some(*contract_scenario)
            }
            _ => None,
        }
    }
}

fn create_account(account_hash: AccountHash, test_scenario: TestScenario) -> Account {
    match test_scenario {
        TestScenario::FromPeerAccountWithInvalidAssociatedKeys(_)
        | TestScenario::FromClientAccountWithInvalidAssociatedKeys(_) => {
            Account::create(AccountHash::default(), NamedKeys::new(), URef::default())
        }
        TestScenario::FromPeerAccountWithInsufficientWeight(_)
        | TestScenario::FromClientAccountWithInsufficientWeight(_) => {
            let invalid_action_threshold =
                ActionThresholds::new(Weight::new(100u8), Weight::new(100u8))
                    .expect("should create action threshold");
            Account::new(
                account_hash,
                NamedKeys::new(),
                URef::default(),
                AssociatedKeys::new(account_hash, Weight::new(1)),
                invalid_action_threshold,
            )
        }
        _ => Account::create(account_hash, NamedKeys::new(), URef::default()),
    }
}

struct Reactor {
    storage: Storage,
    transaction_acceptor: TransactionAcceptor,
    _storage_tempdir: TempDir,
    test_scenario: TestScenario,
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = TestScenario;
    type Error = Error;

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        debug!("{event:?}");
        match event {
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::StorageRequest(req) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            Event::TransactionAcceptor(event) => reactor::wrap_effects(
                Event::TransactionAcceptor,
                self.transaction_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::ControlAnnouncement(ctrl_ann) => {
                panic!("unhandled control announcement: {}", ctrl_ann)
            }
            Event::FatalAnnouncement(fatal_ann) => {
                panic!("unhandled fatal announcement: {}", fatal_ann)
            }
            Event::TransactionAcceptorAnnouncement(_) => {
                // We do not care about transaction acceptor announcements in the acceptor tests.
                Effects::new()
            }
            Event::ContractRuntime(event) => match event {
                ContractRuntimeRequest::Query {
                    request: query_request,
                    responder,
                } => {
                    let query_result =
                        if let Key::Hash(_) | Key::SmartContract(_) = query_request.key() {
                            match self.test_scenario {
                                TestScenario::FromPeerCustomPaymentContractPackage(
                                    ContractPackageScenario::MissingPackageAtHash,
                                )
                                | TestScenario::FromPeerSessionContractPackage(
                                    _,
                                    ContractPackageScenario::MissingPackageAtHash,
                                )
                                | TestScenario::FromClientCustomPaymentContractPackage(
                                    ContractPackageScenario::MissingPackageAtHash,
                                )
                                | TestScenario::FromClientSessionContractPackage(
                                    _,
                                    ContractPackageScenario::MissingPackageAtHash,
                                ) => QueryResult::ValueNotFound(String::new()),
                                TestScenario::FromPeerCustomPaymentContractPackage(
                                    ContractPackageScenario::MissingContractVersion,
                                )
                                | TestScenario::FromPeerSessionContractPackage(
                                    _,
                                    ContractPackageScenario::MissingContractVersion,
                                )
                                | TestScenario::FromClientCustomPaymentContractPackage(
                                    ContractPackageScenario::MissingContractVersion,
                                )
                                | TestScenario::FromClientSessionContractPackage(
                                    _,
                                    ContractPackageScenario::MissingContractVersion,
                                ) => QueryResult::Success {
                                    value: Box::new(StoredValue::ContractPackage(
                                        ContractPackage::default(),
                                    )),
                                    proofs: vec![],
                                },
                                _ => panic!("unexpected query: {:?}", query_request),
                            }
                        } else {
                            panic!("expect only queries using Key::Package variant");
                        };
                    responder.respond(query_result).ignore()
                }
                ContractRuntimeRequest::GetBalance {
                    request: balance_request,
                    responder,
                } => {
                    let key = match balance_request.identifier() {
                        BalanceIdentifier::Purse(uref) => Key::URef(*uref),
                        BalanceIdentifier::Public(public_key) => {
                            Key::Account(public_key.to_account_hash())
                        }
                        BalanceIdentifier::Account(account_hash)
                        | BalanceIdentifier::PenalizedAccount(account_hash) => {
                            Key::Account(*account_hash)
                        }
                        BalanceIdentifier::Entity(entity_addr) => {
                            Key::AddressableEntity(*entity_addr)
                        }
                        BalanceIdentifier::Internal(addr) => Key::Balance(*addr),
                        BalanceIdentifier::Refund => {
                            responder
                                .respond(BalanceResult::Failure(
                                    TrackingCopyError::NamedKeyNotFound("refund".to_string()),
                                ))
                                .ignore::<Self::Event>();
                            return Effects::new();
                        }
                        BalanceIdentifier::Payment | BalanceIdentifier::PenalizedPayment => {
                            responder
                                .respond(BalanceResult::Failure(
                                    TrackingCopyError::NamedKeyNotFound("payment".to_string()),
                                ))
                                .ignore::<Self::Event>();
                            return Effects::new();
                        }
                        BalanceIdentifier::Accumulate => {
                            responder
                                .respond(BalanceResult::Failure(
                                    TrackingCopyError::NamedKeyNotFound("accumulate".to_string()),
                                ))
                                .ignore::<Self::Event>();
                            return Effects::new();
                        }
                    };
                    let purse_addr = match balance_request.identifier().as_purse_addr() {
                        Some(purse_addr) => purse_addr,
                        None => {
                            responder
                                .respond(BalanceResult::Failure(
                                    TrackingCopyError::UnexpectedKeyVariant(key),
                                ))
                                .ignore::<Self::Event>();
                            return Effects::new();
                        }
                    };

                    let proof = TrieMerkleProof::new(
                        key,
                        StoredValue::CLValue(CLValue::from_t(()).expect("should get CLValue")),
                        VecDeque::new(),
                    );
                    let baseline_amount = U512::from(DEFAULT_BASELINE_MOTES_AMOUNT);
                    let motes = if matches!(
                        self.test_scenario,
                        TestScenario::FromClientInsufficientBalance(_)
                    ) {
                        baseline_amount - 1
                    } else {
                        baseline_amount
                    };
                    let balance_result =
                        if self.test_scenario == TestScenario::AccountWithUnknownBalance {
                            BalanceResult::RootNotFound
                        } else {
                            let proofs_result = ProofsResult::Proofs {
                                total_balance_proof: Box::new(proof),
                                balance_holds: Default::default(),
                            };
                            BalanceResult::Success {
                                purse_addr,
                                total_balance: Default::default(),
                                available_balance: motes,
                                proofs_result,
                            }
                        };
                    responder.respond(balance_result).ignore()
                }
                ContractRuntimeRequest::GetAddressableEntity {
                    state_root_hash: _,
                    entity_addr,
                    responder,
                } => {
                    let result = if matches!(
                        self.test_scenario,
                        TestScenario::FromClientMissingAccount(_)
                    ) || matches!(
                        self.test_scenario,
                        TestScenario::FromPeerMissingAccount(_)
                    ) {
                        AddressableEntityResult::ValueNotFound("missing account".to_string())
                    } else if let EntityAddr::Account(account_hash) = entity_addr {
                        let account =
                            create_account(AccountHash::new(account_hash), self.test_scenario);
                        AddressableEntityResult::Success {
                            entity: AddressableEntity::from(account),
                        }
                    } else if let EntityAddr::SmartContract(..) = entity_addr {
                        match self.test_scenario {
                            TestScenario::FromPeerCustomPaymentContract(
                                ContractScenario::MissingContractAtHash,
                            )
                            | TestScenario::FromPeerSessionContract(
                                _,
                                ContractScenario::MissingContractAtHash,
                            )
                            | TestScenario::FromClientCustomPaymentContract(
                                ContractScenario::MissingContractAtHash,
                            )
                            | TestScenario::FromClientSessionContract(
                                _,
                                ContractScenario::MissingContractAtHash,
                            ) => AddressableEntityResult::ValueNotFound(
                                "missing contract".to_string(),
                            ),
                            TestScenario::FromPeerCustomPaymentContract(
                                ContractScenario::MissingEntryPoint,
                            )
                            | TestScenario::FromPeerSessionContract(
                                _,
                                ContractScenario::MissingEntryPoint,
                            )
                            | TestScenario::FromClientCustomPaymentContract(
                                ContractScenario::MissingEntryPoint,
                            )
                            | TestScenario::FromClientSessionContract(
                                _,
                                ContractScenario::MissingEntryPoint,
                            ) => {
                                let contract = Contract::default();
                                AddressableEntityResult::Success {
                                    entity: AddressableEntity::from(contract),
                                }
                            }
                            _ => panic!("unexpected GetAddressableEntity: {:?}", entity_addr),
                        }
                    } else {
                        panic!(
                            "should GetAddressableEntity using Account or SmartContract variant"
                        );
                    };
                    responder.respond(result).ignore()
                }
                ContractRuntimeRequest::GetEntryPointExists {
                    state_root_hash: _,
                    responder,
                    ..
                } => {
                    let contract_scenario = self
                        .test_scenario
                        .contract_scenario()
                        .expect("must get contract scenario");
                    let result = match contract_scenario {
                        ContractScenario::Valid => EntryPointExistsResult::Success,
                        ContractScenario::MissingContractAtHash
                        | ContractScenario::MissingContractAtName
                        | ContractScenario::MissingEntryPoint => {
                            EntryPointExistsResult::ValueNotFound(
                                "entry point not found".to_string(),
                            )
                        }
                    };
                    responder.respond(result).ignore()
                }
                _ => panic!("should not receive {:?}", event),
            },
            Event::NetworkRequest(_) => panic!("test does not handle network requests"),
        }
    }

    fn new(
        config: Self::Config,
        chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        _event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (storage_config, storage_tempdir) = storage::Config::new_for_tests(1);
        let storage_with_dir = WithDir::new(storage_tempdir.path(), storage_config);

        let transaction_acceptor =
            TransactionAcceptor::new(Config::default(), Arc::clone(&chainspec), registry)?;

        let storage = Storage::new(
            &storage_with_dir,
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            EraId::default(),
            "test",
            chainspec.transaction_config.max_ttl.into(),
            chainspec.core_config.recent_era_count(),
            Some(registry),
            false,
            TransactionConfig::default(),
        )
        .unwrap();

        let reactor = Reactor {
            storage,
            transaction_acceptor,
            _storage_tempdir: storage_tempdir,
            test_scenario: config,
        };

        let effects = Effects::new();

        Ok((reactor, effects))
    }
}

fn put_block_to_storage_and_mark_complete(
    block: Arc<BlockV2>,
    result_sender: Sender<bool>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        async move {
            let block_height = block.height();
            let block: Block = (*block).clone().into();
            let result = effect_builder.put_block_to_storage(Arc::new(block)).await;
            effect_builder.mark_block_completed(block_height).await;
            result_sender
                .send(result)
                .expect("receiver should not be dropped yet");
        }
        .ignore()
    }
}

fn put_transaction_to_storage(
    txn: &Transaction,
    result_sender: Sender<bool>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    let txn = txn.clone();
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .put_transaction_to_storage(txn)
            .map(|result| {
                result_sender
                    .send(result)
                    .expect("receiver should not be dropped yet")
            })
            .ignore()
    }
}

fn schedule_accept_transaction(
    txn: &Transaction,
    source: Source,
    responder: Responder<Result<(), super::Error>>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    let transaction = txn.clone();
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .into_inner()
            .schedule(
                super::Event::Accept {
                    transaction,
                    source,
                    maybe_responder: Some(responder),
                },
                QueueKind::Validation,
            )
            .ignore()
    }
}

fn inject_balance_check_for_peer(
    txn: &Transaction,
    source: Source,
    rng: &mut TestRng,
    responder: Responder<Result<(), super::Error>>,
    chainspec: &Chainspec,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    let txn = txn.clone();
    let block = TestBlockBuilder::new().build(rng);
    let block_header = Box::new(block.header().clone().into());
    let meta_transaction =
        MetaTransaction::from_transaction(&txn, &chainspec.transaction_config).unwrap();
    |effect_builder: EffectBuilder<Event>| {
        let event_metadata = Box::new(EventMetadata::new(
            txn,
            meta_transaction,
            source,
            Some(responder),
            Timestamp::now(),
        ));
        effect_builder
            .into_inner()
            .schedule(
                super::Event::GetBalanceResult {
                    event_metadata,
                    block_header,
                    maybe_balance: None,
                },
                QueueKind::ContractRuntime,
            )
            .ignore()
    }
}

async fn run_transaction_acceptor_without_timeout(
    test_scenario: TestScenario,
) -> Result<(), super::Error> {
    let _ = logging::init();
    let rng = &mut TestRng::new();

    let admin = SecretKey::random(rng);
    let (mut chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let mut chainspec = if let TestScenario::TooLowGasPriceToleranceForTransactionV1 = test_scenario
    {
        chainspec.with_pricing_handling(PricingHandling::Fixed);
        chainspec
    } else {
        chainspec
    };
    chainspec.core_config.administrators = iter::once(PublicKey::from(&admin)).collect();

    let chainspec = Arc::new(chainspec);
    let mut runner: Runner<ConditionCheckReactor<Reactor>> = Runner::new(
        test_scenario,
        chainspec.clone(),
        Arc::new(chainspec_raw_bytes),
        rng,
    )
    .await
    .unwrap();

    let block = Arc::new(TestBlockBuilder::new().build(rng));
    // Create a channel to assert that the block was successfully injected into storage.
    let (result_sender, result_receiver) = oneshot::channel();

    runner
        .process_injected_effects(put_block_to_storage_and_mark_complete(block, result_sender))
        .await;

    // There are two scheduled events, so we only need to try cranking until the second time it
    // returns `Some`.
    for _ in 0..2 {
        while runner.try_crank(rng).await == TryCrankOutcome::NoEventsToProcess {
            time::sleep(POLL_INTERVAL).await;
        }
    }
    assert!(result_receiver.await.unwrap());

    // Create a responder to assert the validity of the transaction
    let (txn_sender, txn_receiver) = oneshot::channel();
    let txn_responder = Responder::without_shutdown(txn_sender);

    // Create a transaction specific to the test scenario
    let txn = test_scenario.transaction(rng, &admin);
    // Mark the source as either a peer or a client depending on the scenario.
    let source = test_scenario.source(rng);

    {
        // Inject the transaction artificially into storage to simulate a previously seen one.
        if test_scenario.is_repeated_transaction_case() {
            let (result_sender, result_receiver) = oneshot::channel();
            runner
                .process_injected_effects(put_transaction_to_storage(&txn, result_sender))
                .await;
            while runner.try_crank(rng).await == TryCrankOutcome::NoEventsToProcess {
                time::sleep(POLL_INTERVAL).await;
            }
            // Check that the "previously seen" transaction is present in storage.
            assert!(result_receiver.await.unwrap());
        }

        if test_scenario == TestScenario::BalanceCheckForDeploySentByPeer {
            let (txn_sender, _) = oneshot::channel();
            let txn_responder = Responder::without_shutdown(txn_sender);
            let chainspec = chainspec.as_ref().clone();
            runner
                .process_injected_effects(inject_balance_check_for_peer(
                    &txn,
                    source.clone(),
                    rng,
                    txn_responder,
                    &chainspec,
                ))
                .await;
            while runner.try_crank(rng).await == TryCrankOutcome::NoEventsToProcess {
                time::sleep(POLL_INTERVAL).await;
            }
        }
    }

    runner
        .process_injected_effects(schedule_accept_transaction(&txn, source, txn_responder))
        .await;

    // Tests where the transaction is already in storage will not trigger any transaction acceptor
    // announcement, so use the transaction acceptor `PutToStorage` event as the condition.
    let stopping_condition = move |event: &Event| -> bool {
        match test_scenario {
            // Check that invalid transactions sent by a client raise the `InvalidTransaction`
            // announcement with the appropriate source.
            TestScenario::FromClientInvalidTransaction(_)
            | TestScenario::FromClientFutureDatedTransaction(_)
            | TestScenario::FromClientMissingAccount(_)
            | TestScenario::FromClientInsufficientBalance(_)
            | TestScenario::FromClientAccountWithInvalidAssociatedKeys(_)
            | TestScenario::FromClientAccountWithInsufficientWeight(_)
            | TestScenario::DeployWithEmptySessionModuleBytes
            | TestScenario::AccountWithUnknownBalance
            | TestScenario::DeployWithNativeTransferInPayment
            | TestScenario::DeployWithoutPaymentAmount
            | TestScenario::DeployWithMangledPaymentAmount
            | TestScenario::DeployWithMangledTransferAmount
            | TestScenario::DeployWithoutTransferTarget
            | TestScenario::DeployWithoutTransferAmount
            | TestScenario::InvalidPricingModeForTransactionV1
            | TestScenario::FromClientExpired(_)
            | TestScenario::TooLowGasPriceToleranceForTransactionV1
            | TestScenario::TooLowGasPriceToleranceForDeploy
            | TestScenario::InvalidFields
            | TestScenario::InvalidArgumentsKind
            | TestScenario::WasmTransactionWithTooBigPayment
            | TestScenario::WasmDeployWithTooBigPayment => {
                matches!(
                    event,
                    Event::TransactionAcceptorAnnouncement(
                        TransactionAcceptorAnnouncement::InvalidTransaction {
                            source: Source::Client,
                            ..
                        }
                    )
                )
            }
            // Check that executable items with valid contracts are successfully stored. Conversely,
            // ensure that invalid contracts will raise the invalid transaction announcement.
            TestScenario::FromPeerCustomPaymentContract(contract_scenario)
            | TestScenario::FromPeerSessionContract(_, contract_scenario)
            | TestScenario::FromClientCustomPaymentContract(contract_scenario)
            | TestScenario::FromClientSessionContract(_, contract_scenario) => {
                match contract_scenario {
                    ContractScenario::Valid | ContractScenario::MissingContractAtName => matches!(
                        event,
                        Event::TransactionAcceptorAnnouncement(
                            TransactionAcceptorAnnouncement::AcceptedNewTransaction { .. }
                        )
                    ),
                    ContractScenario::MissingContractAtHash
                    | ContractScenario::MissingEntryPoint => {
                        matches!(
                            event,
                            Event::TransactionAcceptorAnnouncement(
                                TransactionAcceptorAnnouncement::InvalidTransaction { .. }
                            )
                        )
                    }
                }
            }
            // Check that executable items with valid contract packages are successfully stored.
            // Conversely, ensure that invalid contract packages will raise the invalid transaction
            // announcement.
            TestScenario::FromPeerCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromPeerSessionContractPackage(_, contract_package_scenario)
            | TestScenario::FromClientCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromClientSessionContractPackage(_, contract_package_scenario) => {
                match contract_package_scenario {
                    ContractPackageScenario::Valid
                    | ContractPackageScenario::MissingPackageAtName => matches!(
                        event,
                        Event::TransactionAcceptorAnnouncement(
                            TransactionAcceptorAnnouncement::AcceptedNewTransaction { .. }
                        )
                    ),
                    ContractPackageScenario::MissingContractVersion
                    | ContractPackageScenario::MissingPackageAtHash => matches!(
                        event,
                        Event::TransactionAcceptorAnnouncement(
                            TransactionAcceptorAnnouncement::InvalidTransaction { .. }
                        )
                    ),
                }
            }
            // Check that invalid transactions sent by a peer raise the `InvalidTransaction`
            // announcement with the appropriate source.
            TestScenario::FromPeerInvalidTransaction(_)
            | TestScenario::BalanceCheckForDeploySentByPeer
            | TestScenario::InvalidFieldsFromPeer => {
                matches!(
                    event,
                    Event::TransactionAcceptorAnnouncement(
                        TransactionAcceptorAnnouncement::InvalidTransaction {
                            source: Source::Peer(_) | Source::PeerGossiped(_),
                            ..
                        }
                    )
                )
            }
            // Check that a new and valid, transaction sent by a peer raises an
            // `AcceptedNewTransaction` announcement with the appropriate source.
            TestScenario::FromPeerValidTransaction(_)
            | TestScenario::FromPeerMissingAccount(_)
            | TestScenario::FromPeerAccountWithInvalidAssociatedKeys(_)
            | TestScenario::FromPeerAccountWithInsufficientWeight(_)
            | TestScenario::FromPeerExpired(_) => {
                matches!(
                    event,
                    Event::TransactionAcceptorAnnouncement(
                        TransactionAcceptorAnnouncement::AcceptedNewTransaction {
                            source: Source::Peer(_),
                            ..
                        }
                    )
                ) || matches!(
                    event,
                    Event::TransactionAcceptorAnnouncement(
                        TransactionAcceptorAnnouncement::AcceptedNewTransaction {
                            source: Source::PeerGossiped(_),
                            ..
                        }
                    )
                )
            }
            // Check that a new and valid transaction sent by a client raises an
            // `AcceptedNewTransaction` announcement with the appropriate source.
            TestScenario::FromClientValidTransaction(_)
            | TestScenario::FromClientSlightlyFutureDatedTransaction(_)
            | TestScenario::FromClientSignedByAdmin(_) => {
                matches!(
                    event,
                    Event::TransactionAcceptorAnnouncement(
                        TransactionAcceptorAnnouncement::AcceptedNewTransaction {
                            source: Source::Client,
                            ..
                        }
                    )
                )
            }
            // Check that repeated valid transactions from a client raises `PutToStorageResult`
            // with the `is_new` flag as false.
            TestScenario::FromClientRepeatedValidTransaction(_) => matches!(
                event,
                Event::TransactionAcceptor(super::Event::PutToStorageResult { is_new: false, .. })
            ),
            // Check that repeated valid transactions from a peer raises `StoredFinalizedApprovals`
            // with the `is_new` flag as false.
            TestScenario::FromPeerRepeatedValidTransaction(_) => matches!(
                event,
                Event::TransactionAcceptor(super::Event::StoredFinalizedApprovals {
                    is_new: false,
                    ..
                })
            ),
        }
    };
    runner
        .reactor_mut()
        .set_condition_checker(Box::new(stopping_condition));

    loop {
        match runner.try_crank(rng).await {
            TryCrankOutcome::ProcessedAnEvent => {
                if runner.reactor().condition_result() {
                    break;
                }
            }
            TryCrankOutcome::NoEventsToProcess => time::sleep(POLL_INTERVAL).await,
            TryCrankOutcome::ShouldExit(exit_code) => panic!("should not exit: {:?}", exit_code),
            TryCrankOutcome::Exited => unreachable!(),
        }
    }

    {
        // Assert that the transaction is present in the case of a valid transaction.
        // Conversely, assert its absence in the invalid case.
        let is_in_storage = runner
            .reactor()
            .inner()
            .storage
            .get_transaction_by_hash(txn.hash())
            .is_some();

        if test_scenario.is_valid_transaction_case() {
            assert!(is_in_storage)
        } else {
            assert!(!is_in_storage)
        }
    }

    txn_receiver.await.unwrap()
}

async fn run_transaction_acceptor(test_scenario: TestScenario) -> Result<(), super::Error> {
    time::timeout(
        TIMEOUT,
        run_transaction_acceptor_without_timeout(test_scenario),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer() {
    let result =
        run_transaction_acceptor(TestScenario::FromPeerValidTransaction(TxnType::Deploy)).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_transaction_v1_from_peer() {
    let result =
        run_transaction_acceptor(TestScenario::FromPeerValidTransaction(TxnType::V1)).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_invalid_deploy_from_peer() {
    let result =
        run_transaction_acceptor(TestScenario::FromPeerInvalidTransaction(TxnType::Deploy)).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(
            InvalidTransaction::Deploy(_)
        ))
    ))
}

#[tokio::test]
async fn should_reject_invalid_transaction_v1_from_peer() {
    let result =
        run_transaction_acceptor(TestScenario::FromPeerInvalidTransaction(TxnType::V1)).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(_)))
    ))
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer_for_missing_account() {
    let result =
        run_transaction_acceptor(TestScenario::FromPeerMissingAccount(TxnType::Deploy)).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_transaction_v1_from_peer_for_missing_account() {
    let result = run_transaction_acceptor(TestScenario::FromPeerMissingAccount(TxnType::V1)).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer_for_account_with_invalid_associated_keys() {
    let result = run_transaction_acceptor(TestScenario::FromPeerAccountWithInvalidAssociatedKeys(
        TxnType::Deploy,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_transaction_v1_from_peer_for_account_with_invalid_associated_keys() {
    let result = run_transaction_acceptor(TestScenario::FromPeerAccountWithInvalidAssociatedKeys(
        TxnType::V1,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer_for_account_with_insufficient_weight() {
    let result = run_transaction_acceptor(TestScenario::FromPeerAccountWithInsufficientWeight(
        TxnType::Deploy,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_transaction_v1_from_peer_for_account_with_insufficient_weight() {
    let result = run_transaction_acceptor(TestScenario::FromPeerAccountWithInsufficientWeight(
        TxnType::V1,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_deploy_from_client() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientValidTransaction(TxnType::Deploy)).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_transaction_v1_from_client() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientValidTransaction(TxnType::V1)).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_invalid_deploy_from_client() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientInvalidTransaction(TxnType::Deploy)).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(
            InvalidTransaction::Deploy(_)
        ))
    ))
}

#[tokio::test]
async fn should_reject_invalid_transaction_v1_from_client() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientInvalidTransaction(TxnType::V1)).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(_)))
    ))
}

#[tokio::test]
async fn should_accept_slightly_future_dated_deploy_from_client() {
    let result = run_transaction_acceptor(TestScenario::FromClientSlightlyFutureDatedTransaction(
        TxnType::Deploy,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_slightly_future_dated_transaction_v1_from_client() {
    let result = run_transaction_acceptor(TestScenario::FromClientSlightlyFutureDatedTransaction(
        TxnType::V1,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_future_dated_deploy_from_client() {
    let result = run_transaction_acceptor(TestScenario::FromClientFutureDatedTransaction(
        TxnType::Deploy,
    ))
    .await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(
            InvalidTransaction::Deploy(InvalidDeploy::TimestampInFuture { .. })
        ))
    ))
}

#[tokio::test]
async fn should_reject_future_dated_transaction_v1_from_client() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientFutureDatedTransaction(TxnType::V1)).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(
            InvalidTransactionV1::TimestampInFuture { .. }
        )))
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_missing_account() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientMissingAccount(TxnType::Deploy)).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchAddressableEntity { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_transaction_v1_from_client_for_missing_account() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientMissingAccount(TxnType::V1)).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchAddressableEntity { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_account_with_invalid_associated_keys() {
    let result = run_transaction_acceptor(
        TestScenario::FromClientAccountWithInvalidAssociatedKeys(TxnType::Deploy),
    )
    .await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::InvalidAssociatedKeys,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_transaction_v1_from_client_for_account_with_invalid_associated_keys() {
    let result = run_transaction_acceptor(
        TestScenario::FromClientAccountWithInvalidAssociatedKeys(TxnType::V1),
    )
    .await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::InvalidAssociatedKeys,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_account_with_insufficient_weight() {
    let result = run_transaction_acceptor(TestScenario::FromClientAccountWithInsufficientWeight(
        TxnType::Deploy,
    ))
    .await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::InsufficientSignatureWeight,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_transaction_v1_from_client_for_account_with_insufficient_weight() {
    let result = run_transaction_acceptor(TestScenario::FromClientAccountWithInsufficientWeight(
        TxnType::V1,
    ))
    .await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::InsufficientSignatureWeight,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_insufficient_balance() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientInsufficientBalance(TxnType::Deploy))
            .await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::InsufficientBalance { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_transaction_v1_from_client_for_insufficient_balance() {
    let result =
        run_transaction_acceptor(TestScenario::FromClientInsufficientBalance(TxnType::V1)).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::InsufficientBalance { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_unknown_balance() {
    let result = run_transaction_acceptor(TestScenario::AccountWithUnknownBalance).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::UnknownBalance { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_repeated_valid_deploy_from_peer() {
    let result = run_transaction_acceptor(TestScenario::FromPeerRepeatedValidTransaction(
        TxnType::Deploy,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_repeated_valid_transaction_v1_from_peer() {
    let result =
        run_transaction_acceptor(TestScenario::FromPeerRepeatedValidTransaction(TxnType::V1)).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_repeated_valid_deploy_from_client() {
    let result = run_transaction_acceptor(TestScenario::FromClientRepeatedValidTransaction(
        TxnType::Deploy,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_repeated_valid_transaction_v1_from_client() {
    let result = run_transaction_acceptor(TestScenario::FromClientRepeatedValidTransaction(
        TxnType::V1,
    ))
    .await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_valid_custom_payment_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContract(ContractScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_custom_payment_contract_by_name_from_client() {
    let test_scenario =
        TestScenario::FromClientCustomPaymentContract(ContractScenario::MissingContractAtName);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_custom_payment_contract_by_hash_from_client() {
    let test_scenario =
        TestScenario::FromClientCustomPaymentContract(ContractScenario::MissingContractAtHash);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_entry_point_custom_payment_from_client() {
    let test_scenario =
        TestScenario::FromClientCustomPaymentContract(ContractScenario::MissingEntryPoint);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_payment_contract_package_by_name_from_client() {
    let test_scenario =
        TestScenario::FromClientCustomPaymentContractPackage(ContractPackageScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_payment_contract_package_at_name_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContractPackage(
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_payment_contract_package_at_hash_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContractPackage(
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_version_in_payment_contract_package_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContractPackage(
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::MissingEntityAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_session_contract_from_client() {
    let test_scenario =
        TestScenario::FromClientSessionContract(TxnType::Deploy, ContractScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_with_valid_session_contract_from_client() {
    let test_scenario =
        TestScenario::FromClientSessionContract(TxnType::V1, ContractScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_session_contract_by_name_from_client() {
    let test_scenario = TestScenario::FromClientSessionContract(
        TxnType::Deploy,
        ContractScenario::MissingContractAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_with_missing_session_contract_by_name_from_client() {
    let test_scenario = TestScenario::FromClientSessionContract(
        TxnType::V1,
        ContractScenario::MissingContractAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_session_contract_by_hash_from_client() {
    let test_scenario = TestScenario::FromClientSessionContract(
        TxnType::Deploy,
        ContractScenario::MissingContractAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_missing_session_contract_by_hash_from_client() {
    let test_scenario = TestScenario::FromClientSessionContract(
        TxnType::V1,
        ContractScenario::MissingContractAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_entry_point_in_session_contract_from_client() {
    let test_scenario = TestScenario::FromClientSessionContract(
        TxnType::Deploy,
        ContractScenario::MissingEntryPoint,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_missing_entry_point_in_session_contract_from_client() {
    let test_scenario =
        TestScenario::FromClientSessionContract(TxnType::V1, ContractScenario::MissingEntryPoint);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_session_contract_package_from_client() {
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        TxnType::Deploy,
        ContractPackageScenario::Valid,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_with_valid_session_contract_package_from_client() {
    let test_scenario =
        TestScenario::FromClientSessionContractPackage(TxnType::V1, ContractPackageScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_session_contract_package_at_name_from_client() {
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        TxnType::Deploy,
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_with_missing_session_contract_package_at_name_from_client() {
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        TxnType::V1,
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_session_contract_package_at_hash_from_client() {
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        TxnType::Deploy,
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_missing_session_contract_package_at_hash_from_client() {
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        TxnType::V1,
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_version_in_session_contract_package_from_client() {
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        TxnType::Deploy,
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::MissingEntityAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_missing_version_in_session_contract_package_from_client()
{
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        TxnType::V1,
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::MissingEntityAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_custom_payment_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContract(ContractScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_custom_payment_contract_by_name_from_peer() {
    let test_scenario =
        TestScenario::FromPeerCustomPaymentContract(ContractScenario::MissingContractAtName);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_custom_payment_contract_by_hash_from_peer() {
    let test_scenario =
        TestScenario::FromPeerCustomPaymentContract(ContractScenario::MissingContractAtHash);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_entry_point_custom_payment_from_peer() {
    let test_scenario =
        TestScenario::FromPeerCustomPaymentContract(ContractScenario::MissingEntryPoint);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_payment_contract_package_by_name_from_peer() {
    let test_scenario =
        TestScenario::FromPeerCustomPaymentContractPackage(ContractPackageScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_payment_contract_package_at_name_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContractPackage(
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_payment_contract_package_at_hash_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContractPackage(
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_version_in_payment_contract_package_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContractPackage(
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::MissingEntityAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_session_contract_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContract(TxnType::Deploy, ContractScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_with_valid_session_contract_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContract(TxnType::V1, ContractScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_session_contract_by_name_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContract(
        TxnType::Deploy,
        ContractScenario::MissingContractAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_with_missing_session_contract_by_name_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContract(TxnType::V1, ContractScenario::MissingContractAtName);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_session_contract_by_hash_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContract(
        TxnType::Deploy,
        ContractScenario::MissingContractAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_missing_session_contract_by_hash_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContract(TxnType::V1, ContractScenario::MissingContractAtHash);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_entry_point_in_session_contract_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContract(TxnType::Deploy, ContractScenario::MissingEntryPoint);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_missing_entry_point_in_session_contract_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContract(TxnType::V1, ContractScenario::MissingEntryPoint);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_session_contract_package_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContractPackage(
        TxnType::Deploy,
        ContractPackageScenario::Valid,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_with_valid_session_contract_package_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContractPackage(TxnType::V1, ContractPackageScenario::Valid);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_session_contract_package_at_name_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContractPackage(
        TxnType::Deploy,
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_with_missing_session_contract_package_at_name_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContractPackage(
        TxnType::V1,
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_session_contract_package_at_hash_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContractPackage(
        TxnType::Deploy,
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_missing_session_contract_package_at_hash_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContractPackage(
        TxnType::V1,
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::NoSuchPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_version_in_session_contract_package_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContractPackage(
        TxnType::Deploy,
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::MissingEntityAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_missing_version_in_session_contract_package_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContractPackage(
        TxnType::V1,
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::MissingEntityAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_empty_module_bytes_in_session() {
    let test_scenario = TestScenario::DeployWithEmptySessionModuleBytes;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::Deploy(DeployParameterFailure::MissingModuleBytes),
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_transfer_in_payment() {
    let test_scenario = TestScenario::DeployWithNativeTransferInPayment;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::Deploy(DeployParameterFailure::InvalidPaymentVariant),
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_without_payment_amount() {
    let test_scenario = TestScenario::DeployWithoutPaymentAmount;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::Deploy(DeployParameterFailure::MissingPaymentAmount),
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_mangled_payment_amount() {
    let test_scenario = TestScenario::DeployWithMangledPaymentAmount;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::Deploy(DeployParameterFailure::FailedToParsePaymentAmount),
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_without_transfer_amount() {
    let test_scenario = TestScenario::DeployWithoutTransferAmount;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(
            InvalidTransaction::Deploy(InvalidDeploy::MissingTransferAmount)
        ))
    ))
}

#[tokio::test]
async fn should_reject_deploy_without_transfer_target() {
    let test_scenario = TestScenario::DeployWithoutTransferTarget;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::Parameters {
            failure: ParameterFailure::Deploy(DeployParameterFailure::MissingTransferTarget),
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_mangled_transfer_amount() {
    let test_scenario = TestScenario::DeployWithMangledTransferAmount;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(
            InvalidTransaction::Deploy(InvalidDeploy::FailedToParseTransferAmount)
        ))
    ))
}

#[tokio::test]
async fn should_reject_expired_deploy_from_client() {
    let test_scenario = TestScenario::FromClientExpired(TxnType::Deploy);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(result, Err(super::Error::Expired { .. })))
}

#[tokio::test]
async fn should_reject_expired_transaction_v1_from_client() {
    let test_scenario = TestScenario::FromClientExpired(TxnType::V1);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(result, Err(super::Error::Expired { .. })))
}

#[tokio::test]
async fn should_accept_expired_deploy_from_peer() {
    let test_scenario = TestScenario::FromPeerExpired(TxnType::Deploy);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_expired_transaction_v1_from_peer() {
    let test_scenario = TestScenario::FromPeerExpired(TxnType::V1);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
#[should_panic]
async fn should_panic_when_balance_checking_for_deploy_sent_by_peer() {
    let test_scenario = TestScenario::BalanceCheckForDeploySentByPeer;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_signed_by_admin_from_client() {
    let test_scenario = TestScenario::FromClientSignedByAdmin(TxnType::Deploy);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_transaction_v1_signed_by_admin_from_client() {
    let test_scenario = TestScenario::FromClientSignedByAdmin(TxnType::V1);
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_transaction_v1_with_invalid_pricing_mode() {
    let test_scenario = TestScenario::InvalidPricingModeForTransactionV1;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(
            InvalidTransactionV1::InvalidPricingMode { .. }
        )))
    ))
}

#[tokio::test]
async fn should_reject_transaction_v1_with_too_low_gas_price_tolerance() {
    let test_scenario = TestScenario::TooLowGasPriceToleranceForTransactionV1;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(
            InvalidTransactionV1::GasPriceToleranceTooLow { .. }
        )))
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_too_low_gas_price_tolerance() {
    let test_scenario = TestScenario::TooLowGasPriceToleranceForDeploy;
    let result = run_transaction_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(
            InvalidTransaction::Deploy(InvalidDeploy::GasPriceToleranceTooLow { .. })
        ))
    ))
}

#[tokio::test]
async fn should_reject_transaction_with_unexpected_fields() {
    let result = run_transaction_acceptor(TestScenario::InvalidFields).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(
            InvalidTransactionV1::UnexpectedTransactionFieldEntries
        )))
    ))
}

#[tokio::test]
async fn should_reject_transaction_from_peer_with_unexpected_fields() {
    let result = run_transaction_acceptor(TestScenario::InvalidFieldsFromPeer).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(
            InvalidTransactionV1::UnexpectedTransactionFieldEntries
        )))
    ))
}

#[tokio::test]
async fn should_reject_transaction_with_invalid_transaction_args() {
    let result = run_transaction_acceptor(TestScenario::InvalidArgumentsKind).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(
            InvalidTransactionV1::ExpectedNamedArguments
        )))
    ));
}

#[tokio::test]
async fn should_reject_wasm_transaction_with_limited_too_big_payment() {
    let result = run_transaction_acceptor(TestScenario::WasmTransactionWithTooBigPayment).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(InvalidTransaction::V1(
            InvalidTransactionV1::NoWasmLaneMatchesTransaction()
        )))
    ));
}

#[tokio::test]
async fn should_reject_deploy_with_payment_amount_larger_than_max_wasm_lane_limit() {
    let result = run_transaction_acceptor(TestScenario::WasmDeployWithTooBigPayment).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidTransaction(
            InvalidTransaction::Deploy(InvalidDeploy::ExceededWasmLaneGasLimit { .. })
        ))
    ))
}
