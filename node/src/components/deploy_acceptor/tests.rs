#![cfg(test)]

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt::{self, Debug, Display, Formatter},
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

use casper_execution_engine::{
    core::engine_state::{BalanceResult, QueryResult, MAX_PAYMENT_AMOUNT},
    storage::trie::merkle_proof::TrieMerkleProof,
};
use casper_types::{
    account::{Account, ActionThresholds, AssociatedKeys, Weight},
    contracts::NamedKeys,
    testing::TestRng,
    CLValue, EraId, PublicKey, SecretKey, StoredValue, TimeDiff, URef, U512,
};

use super::*;
use crate::{
    components::{
        network::Identity as NetworkIdentity,
        storage::{self, Storage},
    },
    effect::{
        announcements::{ControlAnnouncement, DeployAcceptorAnnouncement},
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
    types::{Block, Chainspec, ChainspecRawBytes, Deploy, NodeId},
    utils::{Loadable, WithDir},
    NodeRng,
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const TIMEOUT: Duration = Duration::from_secs(10);
const ALICE_SECRET_KEY_BYTES: [u8; 32] = [123; 32];
const BOB_SECRET_KEY_BYTES: [u8; 32] = [124; 32];

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[allow(clippy::large_enum_variant)]
#[must_use]
enum Event {
    #[from]
    Storage(#[serde(skip_serializing)] storage::Event),
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] super::Event),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    FatalAnnouncement(FatalAnnouncement),
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement),
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
            Event::DeployAcceptor(event) => write!(formatter, "deploy acceptor: {}", event),
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::FatalAnnouncement(fatal_ann) => write!(formatter, "fatal: {}", fatal_ann),
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(formatter, "deploy-acceptor announcement: {}", ann)
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

#[derive(Clone, PartialEq, Eq, Debug)]
enum TestScenario {
    FromPeerInvalidDeploy,
    FromPeerValidDeploy,
    FromPeerRepeatedValidDeploy,
    FromPeerMissingAccount,
    FromPeerAccountWithInsufficientWeight,
    FromPeerAccountWithInvalidAssociatedKeys,
    FromPeerCustomPaymentContract(ContractScenario),
    FromPeerCustomPaymentContractPackage(ContractPackageScenario),
    FromPeerSessionContract(ContractScenario),
    FromPeerSessionContractPackage(ContractPackageScenario),
    FromClientInvalidDeploy,
    FromClientSlightlyFutureDatedDeploy,
    FromClientFutureDatedDeploy,
    FromClientMissingAccount,
    FromClientInsufficientBalance,
    FromClientValidDeploy,
    FromClientRepeatedValidDeploy,
    FromClientAccountWithInsufficientWeight,
    FromClientAccountWithInvalidAssociatedKeys,
    AccountWithUnknownBalance,
    FromClientCustomPaymentContract(ContractScenario),
    FromClientCustomPaymentContractPackage(ContractPackageScenario),
    FromClientSessionContract(ContractScenario),
    FromClientSessionContractPackage(ContractPackageScenario),
    DeployWithNativeTransferInPayment,
    DeployWithEmptySessionModuleBytes,
    DeployWithoutPaymentAmount,
    DeployWithMangledPaymentAmount,
    DeployWithMangledTransferAmount,
    DeployWithoutTransferTarget,
    DeployWithoutTransferAmount,
    BalanceCheckForDeploySentByPeer,
    ShouldNotAcceptExpiredDeploySentByClient,
    ShouldAcceptExpiredDeploySentByPeer,
    ShouldAcceptDeployFromAdministrator {
        administrators: BTreeSet<PublicKey>,
        secret_keys: Vec<[u8; 32]>,
    },
    ShouldRejectDeployFromNonAdministrator {
        administrators: BTreeSet<PublicKey>,
        secret_keys: Vec<[u8; 32]>,
    },
}

impl TestScenario {
    fn source(&self, rng: &mut NodeRng) -> Source {
        match self {
            TestScenario::FromPeerInvalidDeploy
            | TestScenario::FromPeerValidDeploy
            | TestScenario::FromPeerRepeatedValidDeploy
            | TestScenario::BalanceCheckForDeploySentByPeer
            | TestScenario::FromPeerMissingAccount
            | TestScenario::FromPeerAccountWithInsufficientWeight
            | TestScenario::FromPeerAccountWithInvalidAssociatedKeys
            | TestScenario::FromPeerCustomPaymentContract(_)
            | TestScenario::FromPeerCustomPaymentContractPackage(_)
            | TestScenario::FromPeerSessionContract(_)
            | TestScenario::FromPeerSessionContractPackage(_)
            | TestScenario::ShouldAcceptExpiredDeploySentByPeer => {
                Source::Peer(NodeId::random(rng))
            }
            TestScenario::FromClientInvalidDeploy
            | TestScenario::FromClientSlightlyFutureDatedDeploy
            | TestScenario::FromClientFutureDatedDeploy
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientValidDeploy
            | TestScenario::FromClientRepeatedValidDeploy
            | TestScenario::FromClientAccountWithInsufficientWeight
            | TestScenario::FromClientAccountWithInvalidAssociatedKeys
            | TestScenario::AccountWithUnknownBalance
            | TestScenario::DeployWithoutPaymentAmount
            | TestScenario::DeployWithMangledPaymentAmount
            | TestScenario::DeployWithMangledTransferAmount
            | TestScenario::DeployWithoutTransferAmount
            | TestScenario::DeployWithoutTransferTarget
            | TestScenario::FromClientCustomPaymentContract(_)
            | TestScenario::FromClientCustomPaymentContractPackage(_)
            | TestScenario::FromClientSessionContract(_)
            | TestScenario::FromClientSessionContractPackage(_)
            | TestScenario::DeployWithEmptySessionModuleBytes
            | TestScenario::DeployWithNativeTransferInPayment
            | TestScenario::ShouldNotAcceptExpiredDeploySentByClient
            | TestScenario::ShouldAcceptDeployFromAdministrator { .. }
            | TestScenario::ShouldRejectDeployFromNonAdministrator { .. } => Source::Client,
        }
    }

    fn deploy(&self, rng: &mut NodeRng) -> Deploy {
        match self {
            TestScenario::FromPeerInvalidDeploy | TestScenario::FromClientInvalidDeploy => {
                let mut deploy = Deploy::random_valid_native_transfer(rng);
                deploy.invalidate();
                deploy
            }
            TestScenario::FromPeerValidDeploy
            | TestScenario::FromPeerRepeatedValidDeploy
            | TestScenario::FromPeerMissingAccount
            | TestScenario::FromPeerAccountWithInvalidAssociatedKeys
            | TestScenario::FromPeerAccountWithInsufficientWeight
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientValidDeploy
            | TestScenario::FromClientRepeatedValidDeploy
            | TestScenario::FromClientAccountWithInvalidAssociatedKeys
            | TestScenario::FromClientAccountWithInsufficientWeight
            | TestScenario::AccountWithUnknownBalance
            | TestScenario::BalanceCheckForDeploySentByPeer => {
                Deploy::random_valid_native_transfer(rng)
            }
            TestScenario::DeployWithoutPaymentAmount => Deploy::random_without_payment_amount(rng),
            TestScenario::DeployWithMangledPaymentAmount => {
                Deploy::random_with_mangled_payment_amount(rng)
            }
            TestScenario::DeployWithoutTransferTarget => {
                Deploy::random_without_transfer_target(rng)
            }
            TestScenario::DeployWithoutTransferAmount => {
                Deploy::random_without_transfer_amount(rng)
            }
            TestScenario::DeployWithMangledTransferAmount => {
                Deploy::random_with_mangled_transfer_amount(rng)
            }

            TestScenario::FromPeerCustomPaymentContract(contract_scenario)
            | TestScenario::FromClientCustomPaymentContract(contract_scenario) => {
                match contract_scenario {
                    ContractScenario::Valid | ContractScenario::MissingContractAtName => {
                        Deploy::random_with_valid_custom_payment_contract_by_name(rng)
                    }
                    ContractScenario::MissingEntryPoint => {
                        Deploy::random_with_missing_entry_point_in_payment_contract(rng)
                    }
                    ContractScenario::MissingContractAtHash => {
                        Deploy::random_with_missing_payment_contract_by_hash(rng)
                    }
                }
            }
            TestScenario::FromPeerCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromClientCustomPaymentContractPackage(contract_package_scenario) => {
                match contract_package_scenario {
                    ContractPackageScenario::Valid
                    | ContractPackageScenario::MissingPackageAtName => {
                        Deploy::random_with_valid_custom_payment_package_by_name(rng)
                    }
                    ContractPackageScenario::MissingPackageAtHash => {
                        Deploy::random_with_missing_payment_package_by_hash(rng)
                    }
                    ContractPackageScenario::MissingContractVersion => {
                        Deploy::random_with_nonexistent_contract_version_in_payment_package(rng)
                    }
                }
            }
            TestScenario::FromPeerSessionContract(contract_scenario)
            | TestScenario::FromClientSessionContract(contract_scenario) => match contract_scenario
            {
                ContractScenario::Valid | ContractScenario::MissingContractAtName => {
                    Deploy::random_with_valid_session_contract_by_name(rng)
                }
                ContractScenario::MissingContractAtHash => {
                    Deploy::random_with_missing_session_contract_by_hash(rng)
                }
                ContractScenario::MissingEntryPoint => {
                    Deploy::random_with_missing_entry_point_in_session_contract(rng)
                }
            },
            TestScenario::FromPeerSessionContractPackage(contract_package_scenario)
            | TestScenario::FromClientSessionContractPackage(contract_package_scenario) => {
                match contract_package_scenario {
                    ContractPackageScenario::Valid
                    | ContractPackageScenario::MissingPackageAtName => {
                        Deploy::random_with_valid_session_package_by_name(rng)
                    }
                    ContractPackageScenario::MissingPackageAtHash => {
                        Deploy::random_with_missing_session_package_by_hash(rng)
                    }
                    ContractPackageScenario::MissingContractVersion => {
                        Deploy::random_with_nonexistent_contract_version_in_session_package(rng)
                    }
                }
            }
            TestScenario::DeployWithEmptySessionModuleBytes => {
                Deploy::random_with_empty_session_module_bytes(rng)
            }
            TestScenario::DeployWithNativeTransferInPayment => {
                Deploy::random_with_native_transfer_in_payment_logic(rng)
            }
            TestScenario::ShouldAcceptExpiredDeploySentByPeer
            | TestScenario::ShouldNotAcceptExpiredDeploySentByClient => {
                Deploy::random_expired_deploy(rng)
            }
            TestScenario::ShouldAcceptDeployFromAdministrator { secret_keys, .. }
            | TestScenario::ShouldRejectDeployFromNonAdministrator { secret_keys, .. } => {
                let mut deploy = Deploy::random_with_valid_session_package_by_name(rng);
                deploy.replace_approvals(BTreeSet::new());

                for secret_key_bytes in secret_keys {
                    let secret_key = SecretKey::ed25519_from_bytes(secret_key_bytes)
                        .expect("should create secret key");
                    deploy.sign(&secret_key);
                }

                deploy
            }
            TestScenario::FromClientSlightlyFutureDatedDeploy => {
                let timestamp = Timestamp::now() + (Config::default().timestamp_leeway / 2);
                let ttl = TimeDiff::from_seconds(300);
                Deploy::random_valid_native_transfer_with_timestamp_and_ttl(rng, timestamp, ttl)
            }
            TestScenario::FromClientFutureDatedDeploy => {
                let timestamp = Timestamp::now()
                    + Config::default().timestamp_leeway
                    + TimeDiff::from_millis(100);
                let ttl = TimeDiff::from_seconds(300);
                Deploy::random_valid_native_transfer_with_timestamp_and_ttl(rng, timestamp, ttl)
            }
        }
    }

    fn is_valid_deploy_case(&self) -> bool {
        match self {
            TestScenario::FromPeerRepeatedValidDeploy
            | TestScenario::FromPeerValidDeploy
            | TestScenario::FromPeerMissingAccount // account check skipped if from peer
            | TestScenario::FromPeerAccountWithInsufficientWeight // account check skipped if from peer
            | TestScenario::FromPeerAccountWithInvalidAssociatedKeys // account check skipped if from peer
            | TestScenario::FromClientRepeatedValidDeploy
            | TestScenario::FromClientValidDeploy
            | TestScenario::FromClientSlightlyFutureDatedDeploy
            | TestScenario::ShouldAcceptExpiredDeploySentByPeer=> true,
            TestScenario::FromPeerInvalidDeploy
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInvalidDeploy
            | TestScenario::FromClientFutureDatedDeploy
            | TestScenario::FromClientAccountWithInsufficientWeight
            | TestScenario::FromClientAccountWithInvalidAssociatedKeys
            | TestScenario::AccountWithUnknownBalance
            | TestScenario::DeployWithEmptySessionModuleBytes
            | TestScenario::DeployWithNativeTransferInPayment
            | TestScenario::DeployWithoutPaymentAmount
            | TestScenario::DeployWithMangledPaymentAmount
            | TestScenario::DeployWithMangledTransferAmount
            | TestScenario::DeployWithoutTransferAmount
            | TestScenario::DeployWithoutTransferTarget
            | TestScenario::BalanceCheckForDeploySentByPeer
            | TestScenario::ShouldNotAcceptExpiredDeploySentByClient => false,
            TestScenario::FromPeerCustomPaymentContract(contract_scenario)
            | TestScenario::FromPeerSessionContract(contract_scenario)
            | TestScenario::FromClientCustomPaymentContract(contract_scenario)
            | TestScenario::FromClientSessionContract(contract_scenario) => match contract_scenario
            {
                ContractScenario::Valid
                | ContractScenario::MissingContractAtName => true,
                | ContractScenario::MissingContractAtHash
                | ContractScenario::MissingEntryPoint => false,
            },
            TestScenario::FromPeerCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromPeerSessionContractPackage(contract_package_scenario)
            | TestScenario::FromClientCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromClientSessionContractPackage(contract_package_scenario) => {
                match contract_package_scenario {
                    ContractPackageScenario::Valid
                    | ContractPackageScenario::MissingPackageAtName => true,
                    | ContractPackageScenario::MissingPackageAtHash
                    | ContractPackageScenario::MissingContractVersion => false,
                }
            }
            TestScenario::ShouldAcceptDeployFromAdministrator { .. } => true,
            TestScenario::ShouldRejectDeployFromNonAdministrator { .. } => false
        }
    }

    fn is_repeated_deploy_case(&self) -> bool {
        matches!(
            self,
            TestScenario::FromClientRepeatedValidDeploy | TestScenario::FromPeerRepeatedValidDeploy
        )
    }

    pub(crate) fn create_chainspec_for_test(&self, chainspec: Arc<Chainspec>) -> Arc<Chainspec> {
        match self {
            TestScenario::ShouldAcceptDeployFromAdministrator { administrators, .. }
            | TestScenario::ShouldRejectDeployFromNonAdministrator { administrators, .. } => {
                let mut chainspec = Chainspec {
                    protocol_config: chainspec.protocol_config.clone(),
                    network_config: chainspec.network_config.clone(),
                    core_config: chainspec.core_config.clone(),
                    highway_config: chainspec.highway_config,
                    deploy_config: chainspec.deploy_config,
                    wasm_config: chainspec.wasm_config,
                    system_costs_config: chainspec.system_costs_config,
                };

                chainspec.core_config.administrators.clear();
                chainspec
                    .core_config
                    .administrators
                    .extend(administrators.clone());

                Arc::new(chainspec)
            }
            _ => chainspec,
        }
    }
}

fn create_account(account_hash: AccountHash, test_scenario: &TestScenario) -> Account {
    match test_scenario {
        TestScenario::FromPeerAccountWithInvalidAssociatedKeys
        | TestScenario::FromClientAccountWithInvalidAssociatedKeys => Account::create(
            AccountHash::default(),
            NamedKeys::default(),
            URef::default(),
        ),
        TestScenario::FromPeerAccountWithInsufficientWeight
        | TestScenario::FromClientAccountWithInsufficientWeight => {
            let invalid_action_threshold =
                ActionThresholds::new(Weight::new(100u8), Weight::new(100u8))
                    .expect("should create action threshold");
            Account::new(
                account_hash,
                BTreeMap::new(),
                URef::default(),
                AssociatedKeys::new(account_hash, Weight::new(1)),
                invalid_action_threshold,
            )
        }
        _ => Account::create(account_hash, NamedKeys::default(), URef::default()),
    }
}

struct Reactor {
    storage: Storage,
    deploy_acceptor: DeployAcceptor,
    _storage_tempdir: TempDir,
    test_scenario: TestScenario,
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = TestScenario;
    type Error = Error;

    fn new(
        config: Self::Config,
        chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        _event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (storage_config, storage_tempdir) = storage::Config::default_for_tests();
        let storage_withdir = WithDir::new(storage_tempdir.path(), storage_config);

        let deploy_acceptor = DeployAcceptor::new(
            Config::default(),
            &config.create_chainspec_for_test(Arc::clone(&chainspec)),
            registry,
        )
        .unwrap();

        let storage = Storage::new(
            &storage_withdir,
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            EraId::default(),
            "test",
            chainspec.deploy_config.max_ttl.into(),
            chainspec.core_config.recent_era_count(),
            Some(registry),
            false,
        )
        .unwrap();

        let reactor = Reactor {
            storage,
            deploy_acceptor,
            _storage_tempdir: storage_tempdir,
            test_scenario: config,
        };

        let effects = Effects::new();

        Ok((reactor, effects))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::StorageRequest(req) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, req.into()),
            ),
            Event::DeployAcceptor(event) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::ControlAnnouncement(ctrl_ann) => {
                panic!("unhandled control announcement: {}", ctrl_ann)
            }
            Event::FatalAnnouncement(fatal_ann) => {
                panic!("unhandled fatal announcement: {}", fatal_ann)
            }
            Event::DeployAcceptorAnnouncement(_) => {
                // We do not care about deploy acceptor announcements in the acceptor tests.
                Effects::new()
            }
            Event::ContractRuntime(event) => match event {
                ContractRuntimeRequest::Query {
                    query_request,
                    responder,
                } => {
                    let query_result = if self.test_scenario
                        == TestScenario::FromClientMissingAccount
                        || self.test_scenario == TestScenario::FromPeerMissingAccount
                    {
                        QueryResult::ValueNotFound(String::new())
                    } else if let Key::Account(account_hash) = query_request.key() {
                        if query_request.path().is_empty() {
                            let account = create_account(account_hash, &self.test_scenario);
                            QueryResult::Success {
                                value: Box::new(StoredValue::Account(account)),
                                proofs: vec![],
                            }
                        } else {
                            match self.test_scenario {
                                TestScenario::FromPeerCustomPaymentContractPackage(
                                    contract_package_scenario,
                                )
                                | TestScenario::FromPeerSessionContractPackage(
                                    contract_package_scenario,
                                )
                                | TestScenario::FromClientCustomPaymentContractPackage(
                                    contract_package_scenario,
                                )
                                | TestScenario::FromClientSessionContractPackage(
                                    contract_package_scenario,
                                ) => match contract_package_scenario {
                                    ContractPackageScenario::Valid
                                    | ContractPackageScenario::MissingContractVersion => {
                                        QueryResult::Success {
                                            value: Box::new(StoredValue::ContractPackage(
                                                ContractPackage::default(),
                                            )),
                                            proofs: vec![],
                                        }
                                    }
                                    _ => QueryResult::ValueNotFound(String::new()),
                                },
                                TestScenario::FromPeerSessionContract(contract_scenario)
                                | TestScenario::FromPeerCustomPaymentContract(contract_scenario)
                                | TestScenario::FromClientSessionContract(contract_scenario)
                                | TestScenario::FromClientCustomPaymentContract(
                                    contract_scenario,
                                ) => match contract_scenario {
                                    ContractScenario::Valid
                                    | ContractScenario::MissingEntryPoint => QueryResult::Success {
                                        value: Box::new(StoredValue::Contract(Contract::default())),
                                        proofs: vec![],
                                    },
                                    _ => QueryResult::ValueNotFound(String::new()),
                                },
                                _ => QueryResult::ValueNotFound(String::new()),
                            }
                        }
                    } else if let Key::Hash(_) = query_request.key() {
                        match self.test_scenario {
                            TestScenario::FromPeerSessionContract(contract_scenario)
                            | TestScenario::FromPeerCustomPaymentContract(contract_scenario)
                            | TestScenario::FromClientSessionContract(contract_scenario)
                            | TestScenario::FromClientCustomPaymentContract(contract_scenario) => {
                                match contract_scenario {
                                    ContractScenario::Valid
                                    | ContractScenario::MissingEntryPoint => QueryResult::Success {
                                        value: Box::new(StoredValue::Contract(Contract::default())),
                                        proofs: vec![],
                                    },
                                    ContractScenario::MissingContractAtHash
                                    | ContractScenario::MissingContractAtName => {
                                        QueryResult::ValueNotFound(String::new())
                                    }
                                }
                            }
                            TestScenario::FromPeerSessionContractPackage(
                                contract_package_scenario,
                            )
                            | TestScenario::FromPeerCustomPaymentContractPackage(
                                contract_package_scenario,
                            )
                            | TestScenario::FromClientSessionContractPackage(
                                contract_package_scenario,
                            )
                            | TestScenario::FromClientCustomPaymentContractPackage(
                                contract_package_scenario,
                            ) => match contract_package_scenario {
                                ContractPackageScenario::Valid
                                | ContractPackageScenario::MissingContractVersion => {
                                    QueryResult::Success {
                                        value: Box::new(StoredValue::ContractPackage(
                                            ContractPackage::default(),
                                        )),
                                        proofs: vec![],
                                    }
                                }
                                ContractPackageScenario::MissingPackageAtHash
                                | ContractPackageScenario::MissingPackageAtName => {
                                    QueryResult::ValueNotFound(String::new())
                                }
                            },
                            _ => QueryResult::ValueNotFound(String::new()),
                        }
                    } else {
                        panic!("expect only queries using Key::Account or Key::Hash variant");
                    };
                    responder.respond(Ok(query_result)).ignore()
                }
                ContractRuntimeRequest::GetBalance {
                    balance_request,
                    responder,
                } => {
                    let proof = TrieMerkleProof::new(
                        balance_request.purse_uref().into(),
                        StoredValue::CLValue(CLValue::from_t(()).expect("should get CLValue")),
                        VecDeque::new(),
                    );
                    let motes = if self.test_scenario == TestScenario::FromClientInsufficientBalance
                    {
                        MAX_PAYMENT_AMOUNT - 1
                    } else {
                        MAX_PAYMENT_AMOUNT
                    };
                    let balance_result =
                        if self.test_scenario == TestScenario::AccountWithUnknownBalance {
                            BalanceResult::RootNotFound
                        } else {
                            BalanceResult::Success {
                                motes: U512::from(motes),
                                proof: Box::new(proof),
                            }
                        };
                    responder.respond(Ok(balance_result)).ignore()
                }
                _ => panic!("should not receive {:?}", event),
            },
            Event::NetworkRequest(_) => panic!("test does not handle network requests"),
        }
    }
}

fn put_block_to_storage_and_mark_complete(
    block: Arc<Block>,
    result_sender: Sender<bool>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        async move {
            let block_height = block.height();
            let result = effect_builder.put_block_to_storage(block).await;
            effect_builder.mark_block_completed(block_height).await;
            result_sender
                .send(result)
                .expect("receiver should not be dropped yet");
        }
        .ignore()
    }
}

fn put_deploy_to_storage(
    deploy: Arc<Deploy>,
    result_sender: Sender<bool>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .put_deploy_to_storage(deploy)
            .map(|result| {
                result_sender
                    .send(result)
                    .expect("receiver should not be dropped yet")
            })
            .ignore()
    }
}

fn schedule_accept_deploy(
    deploy: Arc<Deploy>,
    source: Source,
    responder: Responder<Result<(), super::Error>>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .into_inner()
            .schedule(
                super::Event::Accept {
                    deploy,
                    source,
                    maybe_responder: Some(responder),
                },
                QueueKind::Validation,
            )
            .ignore()
    }
}

fn inject_balance_check_for_peer(
    deploy: Arc<Deploy>,
    source: Source,
    rng: &mut TestRng,
    responder: Responder<Result<(), super::Error>>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    let block_header = Box::new(Block::random(rng).header().clone());
    |effect_builder: EffectBuilder<Event>| {
        let event_metadata = Box::new(EventMetadata::new(deploy, source, Some(responder)));
        effect_builder
            .into_inner()
            .schedule(
                super::Event::GetBalanceResult {
                    event_metadata,
                    block_header,
                    maybe_balance_value: None,
                    account_hash: Default::default(),
                    verification_start_timestamp: Timestamp::now(),
                },
                QueueKind::ContractRuntime,
            )
            .ignore()
    }
}

async fn run_deploy_acceptor_without_timeout(
    test_scenario: TestScenario,
) -> Result<(), super::Error> {
    let _ = logging::init();
    let mut rng = crate::new_rng();

    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");

    let mut runner: Runner<ConditionCheckReactor<Reactor>> = Runner::new(
        test_scenario.clone(),
        Arc::new(chainspec),
        Arc::new(chainspec_raw_bytes),
        &mut rng,
    )
    .await
    .unwrap();

    let block = Arc::new(Block::random(&mut rng));
    // Create a channel to assert that the block was successfully injected into storage.
    let (result_sender, result_receiver) = oneshot::channel();

    runner
        .process_injected_effects(put_block_to_storage_and_mark_complete(block, result_sender))
        .await;

    // There are two scheduled events, so we only need to try cranking until the second time it
    // returns `Some`.
    for _ in 0..2 {
        while runner.try_crank(&mut rng).await == TryCrankOutcome::NoEventsToProcess {
            time::sleep(POLL_INTERVAL).await;
        }
    }
    assert!(result_receiver.await.unwrap());

    // Create a responder to assert the validity of the deploy
    let (deploy_sender, deploy_receiver) = oneshot::channel();
    let deploy_responder = Responder::without_shutdown(deploy_sender);

    // Create a deploy specific to the test scenario
    let deploy = Arc::new(test_scenario.deploy(&mut rng));
    // Mark the source as either a peer or a client depending on the scenario.
    let source = test_scenario.source(&mut rng);

    {
        // Inject the deploy artificially into storage to simulate a previously seen deploy.
        if test_scenario.is_repeated_deploy_case() {
            let injected_deploy = Arc::clone(&deploy);
            let (result_sender, result_receiver) = oneshot::channel();
            runner
                .process_injected_effects(put_deploy_to_storage(injected_deploy, result_sender))
                .await;
            while runner.try_crank(&mut rng).await == TryCrankOutcome::NoEventsToProcess {
                time::sleep(POLL_INTERVAL).await;
            }
            // Check that the "previously seen" deploy is present in storage.
            assert!(result_receiver.await.unwrap());
        }

        if test_scenario == TestScenario::BalanceCheckForDeploySentByPeer {
            let fatal_deploy = Arc::clone(&deploy);
            let (deploy_sender, _) = oneshot::channel();
            let deploy_responder = Responder::without_shutdown(deploy_sender);
            runner
                .process_injected_effects(inject_balance_check_for_peer(
                    fatal_deploy,
                    source.clone(),
                    &mut rng,
                    deploy_responder,
                ))
                .await;
            while runner.try_crank(&mut rng).await == TryCrankOutcome::NoEventsToProcess {
                time::sleep(POLL_INTERVAL).await;
            }
        }
    }

    runner
        .process_injected_effects(schedule_accept_deploy(
            Arc::clone(&deploy),
            source,
            deploy_responder,
        ))
        .await;

    // Tests where the deploy is already in storage will not trigger any deploy acceptor
    // announcement, so use the deploy acceptor `PutToStorage` event as the condition.
    let test_scenario_cloned = test_scenario.clone();
    let stopping_condition = move |event: &Event| -> bool {
        match test_scenario_cloned {
            // Check that invalid deploys sent by a client raise the `InvalidDeploy` announcement
            // with the appropriate source.
            TestScenario::FromClientInvalidDeploy
            | TestScenario::FromClientFutureDatedDeploy
            | TestScenario::FromClientMissingAccount
            | TestScenario::FromClientInsufficientBalance
            | TestScenario::FromClientAccountWithInvalidAssociatedKeys
            | TestScenario::FromClientAccountWithInsufficientWeight
            | TestScenario::DeployWithEmptySessionModuleBytes
            | TestScenario::AccountWithUnknownBalance
            | TestScenario::DeployWithNativeTransferInPayment
            | TestScenario::DeployWithoutPaymentAmount
            | TestScenario::DeployWithMangledPaymentAmount
            | TestScenario::DeployWithMangledTransferAmount
            | TestScenario::DeployWithoutTransferTarget
            | TestScenario::DeployWithoutTransferAmount
            | TestScenario::ShouldNotAcceptExpiredDeploySentByClient => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                        source: Source::Client,
                        ..
                    })
                )
            }
            // Check that executable items with valid contracts are successfully stored. Conversely,
            // ensure that invalid contracts will raise the invalid deploy announcement.
            TestScenario::FromPeerCustomPaymentContract(contract_scenario)
            | TestScenario::FromPeerSessionContract(contract_scenario)
            | TestScenario::FromClientCustomPaymentContract(contract_scenario)
            | TestScenario::FromClientSessionContract(contract_scenario) => match contract_scenario
            {
                ContractScenario::Valid | ContractScenario::MissingContractAtName => matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy { .. }
                    )
                ),
                ContractScenario::MissingContractAtHash | ContractScenario::MissingEntryPoint => {
                    matches!(
                        event,
                        Event::DeployAcceptorAnnouncement(
                            DeployAcceptorAnnouncement::InvalidDeploy { .. }
                        )
                    )
                }
            },
            // Check that executable items with valid contract packages are successfully stored.
            // Conversely, ensure that invalid contract packages will raise the invalid deploy
            // announcement.
            TestScenario::FromPeerCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromPeerSessionContractPackage(contract_package_scenario)
            | TestScenario::FromClientCustomPaymentContractPackage(contract_package_scenario)
            | TestScenario::FromClientSessionContractPackage(contract_package_scenario) => {
                match contract_package_scenario {
                    ContractPackageScenario::Valid
                    | ContractPackageScenario::MissingPackageAtName => matches!(
                        event,
                        Event::DeployAcceptorAnnouncement(
                            DeployAcceptorAnnouncement::AcceptedNewDeploy { .. }
                        )
                    ),
                    ContractPackageScenario::MissingContractVersion
                    | ContractPackageScenario::MissingPackageAtHash => matches!(
                        event,
                        Event::DeployAcceptorAnnouncement(
                            DeployAcceptorAnnouncement::InvalidDeploy { .. }
                        )
                    ),
                }
            }
            // Check that invalid deploys sent by a peer raise the `InvalidDeploy` announcement
            // with the appropriate source.
            TestScenario::FromPeerInvalidDeploy | TestScenario::BalanceCheckForDeploySentByPeer => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(DeployAcceptorAnnouncement::InvalidDeploy {
                        source: Source::Peer(_) | Source::PeerGossiped(_),
                        ..
                    })
                )
            }
            // Check that a, new and valid, deploy sent by a peer raises an `AcceptedNewDeploy`
            // announcement with the appropriate source.
            TestScenario::FromPeerValidDeploy
            | TestScenario::FromPeerMissingAccount
            | TestScenario::FromPeerAccountWithInvalidAssociatedKeys
            | TestScenario::FromPeerAccountWithInsufficientWeight
            | TestScenario::ShouldAcceptExpiredDeploySentByPeer => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy {
                            source: Source::Peer(_),
                            ..
                        }
                    )
                ) || matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy {
                            source: Source::PeerGossiped(_),
                            ..
                        }
                    )
                )
            }
            // Check that a, new and valid, deploy sent by a client raises an `AcceptedNewDeploy`
            // announcement with the appropriate source.
            TestScenario::FromClientValidDeploy
            | TestScenario::FromClientSlightlyFutureDatedDeploy => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy {
                            source: Source::Client,
                            ..
                        }
                    )
                )
            }
            // Check that repeated valid deploys from a client raises `PutToStorageResult` with the
            // `is_new` flag as false.
            TestScenario::FromClientRepeatedValidDeploy => matches!(
                event,
                Event::DeployAcceptor(super::Event::PutToStorageResult { is_new: false, .. })
            ),
            // Check that repeated valid deploys from a peer raises `StoredFinalizedApprovals` with
            // the `is_new` flag as false.
            TestScenario::FromPeerRepeatedValidDeploy => matches!(
                event,
                Event::DeployAcceptor(super::Event::StoredFinalizedApprovals { is_new: false, .. })
            ),
            TestScenario::ShouldAcceptDeployFromAdministrator { .. } => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::AcceptedNewDeploy {
                            source: Source::Client,
                            ..
                        }
                    )
                )
            }
            TestScenario::ShouldRejectDeployFromNonAdministrator { .. } => {
                matches!(
                    event,
                    Event::DeployAcceptorAnnouncement(
                        DeployAcceptorAnnouncement::InvalidDeploy { .. }
                    )
                )
            }
        }
    };
    runner
        .reactor_mut()
        .set_condition_checker(Box::new(stopping_condition));

    loop {
        match runner.try_crank(&mut rng).await {
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
        // Assert that the deploy is present in the case of a valid deploy.
        // Conversely, assert its absence in the invalid case.
        let is_in_storage = runner
            .reactor()
            .inner()
            .storage
            .get_deploy_by_hash(*deploy.hash())
            .is_some();

        if test_scenario.is_valid_deploy_case() {
            assert!(is_in_storage)
        } else {
            assert!(!is_in_storage)
        }
    }

    deploy_receiver.await.unwrap()
}

async fn run_deploy_acceptor(test_scenario: TestScenario) -> Result<(), super::Error> {
    time::timeout(TIMEOUT, run_deploy_acceptor_without_timeout(test_scenario))
        .await
        .unwrap()
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer() {
    let result = run_deploy_acceptor(TestScenario::FromPeerValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_invalid_deploy_from_peer() {
    let result = run_deploy_acceptor(TestScenario::FromPeerInvalidDeploy).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployConfiguration(_))
    ))
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer_for_missing_account() {
    let result = run_deploy_acceptor(TestScenario::FromPeerMissingAccount).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer_for_account_with_invalid_associated_keys() {
    let result = run_deploy_acceptor(TestScenario::FromPeerAccountWithInvalidAssociatedKeys).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer_for_account_with_insufficient_weight() {
    let result = run_deploy_acceptor(TestScenario::FromPeerAccountWithInsufficientWeight).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_deploy_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_invalid_deploy_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientInvalidDeploy).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployConfiguration(_))
    ))
}

#[tokio::test]
async fn should_accept_slightly_future_dated_deploy_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientSlightlyFutureDatedDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_future_dated_deploy_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientFutureDatedDeploy).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployConfiguration(
            DeployConfigurationFailure::TimestampInFuture { .. }
        ))
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_missing_account() {
    let result = run_deploy_acceptor(TestScenario::FromClientMissingAccount).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentAccount { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_account_with_invalid_associated_keys() {
    let result =
        run_deploy_acceptor(TestScenario::FromClientAccountWithInvalidAssociatedKeys).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::InvalidAssociatedKeys,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_account_with_insufficient_weight() {
    let result = run_deploy_acceptor(TestScenario::FromClientAccountWithInsufficientWeight).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::InsufficientDeploySignatureWeight,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_insufficient_balance() {
    let result = run_deploy_acceptor(TestScenario::FromClientInsufficientBalance).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::InsufficientBalance { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_valid_deploy_from_client_for_unknown_balance() {
    let result = run_deploy_acceptor(TestScenario::AccountWithUnknownBalance).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::UnknownBalance { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_repeated_valid_deploy_from_peer() {
    let result = run_deploy_acceptor(TestScenario::FromPeerRepeatedValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_repeated_valid_deploy_from_client() {
    let result = run_deploy_acceptor(TestScenario::FromClientRepeatedValidDeploy).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_valid_custom_payment_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContract(ContractScenario::Valid);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_custom_payment_contract_by_name_from_client() {
    let test_scenario =
        TestScenario::FromClientCustomPaymentContract(ContractScenario::MissingContractAtName);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_custom_payment_contract_by_hash_from_client() {
    let test_scenario =
        TestScenario::FromClientCustomPaymentContract(ContractScenario::MissingContractAtHash);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_entry_point_custom_payment_from_client() {
    let test_scenario =
        TestScenario::FromClientCustomPaymentContract(ContractScenario::MissingEntryPoint);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_payment_contract_package_by_name_from_client() {
    let test_scenario =
        TestScenario::FromClientCustomPaymentContractPackage(ContractPackageScenario::Valid);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_payment_contract_package_at_name_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContractPackage(
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_payment_contract_package_at_hash_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContractPackage(
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_version_in_payment_contract_package_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContractPackage(
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::InvalidContractAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_session_contract_from_client() {
    let test_scenario = TestScenario::FromClientSessionContract(ContractScenario::Valid);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_session_contract_by_name_from_client() {
    let test_scenario =
        TestScenario::FromClientSessionContract(ContractScenario::MissingContractAtName);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_session_contract_by_hash_from_client() {
    let test_scenario =
        TestScenario::FromClientSessionContract(ContractScenario::MissingContractAtHash);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_entry_point_in_session_contract_from_client() {
    let test_scenario =
        TestScenario::FromClientSessionContract(ContractScenario::MissingEntryPoint);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_session_contract_package_from_client() {
    let test_scenario =
        TestScenario::FromClientSessionContractPackage(ContractPackageScenario::Valid);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_session_contract_package_at_name_from_client() {
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_session_contract_package_at_hash_from_client() {
    let test_scenario = TestScenario::FromClientSessionContractPackage(
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_version_in_session_contract_package_from_client() {
    let test_scenario = TestScenario::FromClientCustomPaymentContractPackage(
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::InvalidContractAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_custom_payment_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContract(ContractScenario::Valid);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_custom_payment_contract_by_name_from_peer() {
    let test_scenario =
        TestScenario::FromPeerCustomPaymentContract(ContractScenario::MissingContractAtName);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_custom_payment_contract_by_hash_from_peer() {
    let test_scenario =
        TestScenario::FromPeerCustomPaymentContract(ContractScenario::MissingContractAtHash);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_entry_point_custom_payment_from_peer() {
    let test_scenario =
        TestScenario::FromPeerCustomPaymentContract(ContractScenario::MissingEntryPoint);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_payment_contract_package_by_name_from_peer() {
    let test_scenario =
        TestScenario::FromPeerCustomPaymentContractPackage(ContractPackageScenario::Valid);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_payment_contract_package_at_name_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContractPackage(
        ContractPackageScenario::MissingPackageAtName,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_payment_contract_package_at_hash_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContractPackage(
        ContractPackageScenario::MissingPackageAtHash,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_version_in_payment_contract_package_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContractPackage(
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::InvalidContractAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_session_contract_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContract(ContractScenario::Valid);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_session_contract_by_name_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContract(ContractScenario::MissingContractAtName);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_session_contract_by_hash_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContract(ContractScenario::MissingContractAtHash);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_entry_point_in_session_contract_from_peer() {
    let test_scenario = TestScenario::FromPeerSessionContract(ContractScenario::MissingEntryPoint);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractEntryPoint { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_accept_deploy_with_valid_session_contract_package_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContractPackage(ContractPackageScenario::Valid);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_deploy_with_missing_session_contract_package_at_name_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContractPackage(ContractPackageScenario::MissingPackageAtName);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_deploy_with_missing_session_contract_package_at_hash_from_peer() {
    let test_scenario =
        TestScenario::FromPeerSessionContractPackage(ContractPackageScenario::MissingPackageAtHash);
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::NonexistentContractPackageAtHash { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_missing_version_in_session_contract_package_from_peer() {
    let test_scenario = TestScenario::FromPeerCustomPaymentContractPackage(
        ContractPackageScenario::MissingContractVersion,
    );
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::InvalidContractAtVersion { .. },
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_empty_module_bytes_in_session() {
    let test_scenario = TestScenario::DeployWithEmptySessionModuleBytes;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::MissingModuleBytes,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_transfer_in_payment() {
    let test_scenario = TestScenario::DeployWithNativeTransferInPayment;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::InvalidPaymentVariant,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_without_payment_amount() {
    let test_scenario = TestScenario::DeployWithoutPaymentAmount;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::MissingPaymentAmount,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_mangled_payment_amount() {
    let test_scenario = TestScenario::DeployWithMangledPaymentAmount;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::FailedToParsePaymentAmount,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_without_transfer_amount() {
    let test_scenario = TestScenario::DeployWithoutTransferAmount;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployConfiguration(
            DeployConfigurationFailure::MissingTransferAmount
        ))
    ))
}

#[tokio::test]
async fn should_reject_deploy_without_transfer_target() {
    let test_scenario = TestScenario::DeployWithoutTransferTarget;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployParameters {
            failure: DeployParameterFailure::MissingTransferTarget,
            ..
        })
    ))
}

#[tokio::test]
async fn should_reject_deploy_with_mangled_transfer_amount() {
    let test_scenario = TestScenario::DeployWithMangledTransferAmount;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(
        result,
        Err(super::Error::InvalidDeployConfiguration(
            DeployConfigurationFailure::FailedToParseTransferAmount
        ))
    ))
}

#[tokio::test]
async fn should_reject_expired_deploy_from_client() {
    let test_scenario = TestScenario::ShouldNotAcceptExpiredDeploySentByClient;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(matches!(result, Err(super::Error::ExpiredDeploy { .. })))
}

#[tokio::test]
async fn should_accept_expired_deploy_from_peer() {
    let test_scenario = TestScenario::ShouldAcceptExpiredDeploySentByPeer;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
#[should_panic]
async fn should_panic_when_balance_checking_for_deploy_sent_by_peer() {
    let test_scenario = TestScenario::BalanceCheckForDeploySentByPeer;
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_accept_valid_deploy_from_peer_signed_by_administrator() {
    let alice = SecretKey::ed25519_from_bytes(ALICE_SECRET_KEY_BYTES).expect("should create alice");
    let bob = SecretKey::ed25519_from_bytes(BOB_SECRET_KEY_BYTES).expect("should create alice");

    let test_scenario = TestScenario::ShouldAcceptDeployFromAdministrator {
        administrators: [alice, bob].iter().map(PublicKey::from).collect(),
        secret_keys: vec![ALICE_SECRET_KEY_BYTES],
    };
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_ok())
}

#[tokio::test]
async fn should_reject_valid_deploy_from_peer_signed_by_non_administrator() {
    let alice = SecretKey::ed25519_from_bytes(ALICE_SECRET_KEY_BYTES).expect("should create alice");

    let test_scenario = TestScenario::ShouldRejectDeployFromNonAdministrator {
        administrators: [alice].iter().map(PublicKey::from).collect(),
        secret_keys: vec![BOB_SECRET_KEY_BYTES],
    };
    let result = run_deploy_acceptor(test_scenario).await;
    assert!(result.is_err())
}
