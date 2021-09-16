mod config;
mod event;
mod metrics;
mod tests;

use std::fmt::Debug;

use datasize::DataSize;
use prometheus::Registry;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error};

use casper_execution_engine::core::engine_state::{
    executable_deploy_item::{
        ContractIdentifier, ContractPackageIdentifier, ExecutableDeployItemIdentifier,
    },
    ExecutableDeployItem, MAX_PAYMENT,
};
use casper_hashing::Digest;
use casper_types::{
    account::{Account, AccountHash},
    system::auction::ARG_AMOUNT,
    Contract, ContractHash, ContractPackage, ContractPackageHash, ContractVersion,
    ContractVersionKey, Key, ProtocolVersion, U512,
};

use crate::{
    components::Component,
    effect::{
        announcements::DeployAcceptorAnnouncement,
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{
        chainspec::DeployConfig, Block, Chainspec, Deploy, DeployConfigurationFailure, NodeId,
        Timestamp,
    },
    utils::Source,
    NodeRng,
};

pub(crate) use config::Config;
pub(crate) use event::{Event, EventMetadata};

#[derive(Debug, Error, Serialize)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Error {
    /// An invalid deploy was received from the client.
    #[error("block chain has no blocks")]
    InvalidBlockchain,

    /// An invalid deploy was received from the client.
    #[error("invalid deploy: {0}")]
    InvalidDeployConfiguration(DeployConfigurationFailure),

    /// An invalid deploy was received from the client.
    #[error("deploy parameter failure: {failure} at prestate_hash: {prestate_hash}")]
    InvalidDeployParameters {
        prestate_hash: Digest,
        failure: DeployParameterFailure,
    },
}

/// A representation of the way in which a deploy failed validation checks.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
pub enum DeployParameterFailure {
    /// Account does not exist
    #[error("Account does not exist")]
    NonexistentAccount { account_hash: AccountHash },
    /// Nonexistent contract at hash
    #[error("Contract at {contract_hash} does not exist")]
    NonexistentContractAtHash { contract_hash: ContractHash },
    #[error("Contract named {name} does not exist in Account's NamedKeys")]
    NonexistentContractAtName { name: String },
    /// Nonexistent contract entrypoint
    #[error("Contract does not have {entry_point}")]
    NonexistentContractEntryPoint { entry_point: String },
    /// Contract Package does not exist
    #[error("Contract Package at {contract_package_hash} does not exist")]
    NonexistentContractPackageAtHash {
        contract_package_hash: ContractPackageHash,
    },
    #[error("ContractPackage named {name} does not exist in Account's NamedKeys")]
    NonexistentContractPackageAtName { name: String },
    /// Invalid contract at given version.
    #[error("Invalid contract at version: {contract_version}")]
    InvalidContractAtVersion { contract_version: ContractVersion },
    /// Invalid associated keys
    #[error("Account authorization invalid")]
    InvalidAssociatedKeys,
    /// Insufficient deploy signature weight
    #[error("Insufficient deploy signature weight")]
    InsufficientDeploySignatureWeight,
    /// A deploy was sent from account with insufficient balance.
    #[error("Insufficient balance")]
    InsufficientBalance,
    /// A deploy was sent from account with an unknown balance.
    #[error("Unable to determine balance")]
    UnknownBalance,
    /// Transfer is not valid for payment code.
    #[error("Transfer is not valid for payment code")]
    InvalidPaymentVariant,
    /// Missing payment amount.
    #[error("missing payment argument amount")]
    MissingPaymentAmount,
    /// Failed to parse payment amount.
    #[error("failed to parse payment amount as U512")]
    FailedToParsePaymentAmount,
    /// Failed to parse transfer amount.
    #[error("failed to parse transfer amount as U512")]
    FailedToParseTransferAmount,
    /// Missing transfer amount.
    #[error("missing transfer amount")]
    MissingTransferAmount,
    /// Missing transfer target argument.
    #[error("missing transfer target argument")]
    MissingTransferTargetArgument,
    /// Missing transfer source argument.
    #[error("missing transfer source argument")]
    MissingTransferSourceArgument,
    /// Module bytes for session code cannot be empty.
    #[error("module bytes for session code cannot be empty")]
    MissingModuleBytes,
}

/// A helper trait constraining `DeployAcceptor` compatible reactor events.
pub(crate) trait ReactorEventT:
    From<Event>
    + From<DeployAcceptorAnnouncement<NodeId>>
    + From<StorageRequest>
    + From<ContractRuntimeRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<DeployAcceptorAnnouncement<NodeId>>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + Send
{
}

/// The `DeployAcceptor` is the component which handles all new `Deploy`s immediately after they're
/// received by this node, regardless of whether they were provided by a peer or a client.
///
/// It validates a new `Deploy` as far as possible, stores it if valid, then announces the newly-
/// accepted `Deploy`.
#[derive(Debug)]
pub struct DeployAcceptor {
    chain_name: String,
    protocol_version: ProtocolVersion,
    deploy_config: DeployConfig,
    verify_accounts: bool,
    metrics: metrics::Metrics,
}

impl DeployAcceptor {
    pub(crate) fn new(
        config: Config,
        chainspec: &Chainspec,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(DeployAcceptor {
            chain_name: chainspec.network_config.name.clone(),
            protocol_version: chainspec.protocol_version(),
            deploy_config: chainspec.deploy_config,
            verify_accounts: config.verify_accounts(),
            metrics: metrics::Metrics::new(registry)?,
        })
    }

    fn accept<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let verification_start_timestamp = Timestamp::now();
        let mut cloned_deploy = deploy.clone();
        let acceptable_result =
            cloned_deploy.is_config_compliant(&self.chain_name, &self.deploy_config);
        // checks chainspec values
        // DOES NOT check cryptographic security
        if let Err(error) = acceptable_result {
            return effect_builder
                .immediately()
                .event(move |_| Event::InvalidDeployResult {
                    event_metadata: EventMetadata::new(deploy, source, maybe_responder),
                    error: Error::InvalidDeployConfiguration(error),
                    verification_start_timestamp,
                });
        }

        if self.verify_accounts {
            effect_builder
                .get_highest_block_from_storage()
                .event(move |maybe_block| Event::GetBlockResult {
                    event_metadata: EventMetadata::new(deploy, source, maybe_responder),
                    maybe_block: Box::new(maybe_block),
                    verification_start_timestamp,
                })
        } else {
            effect_builder
                .immediately()
                .event(move |_| Event::VerifyDeployCryptographicValidity {
                    event_metadata: EventMetadata::new(deploy, source, maybe_responder),
                    verification_start_timestamp,
                })
        }
    }

    fn handle_get_block_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        maybe_block: Option<Block>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let mut effects = Effects::new();

        let block = match maybe_block {
            Some(block) => block,
            None => {
                // this should be unreachable per current design of the system
                if let Some(responder) = event_metadata.take_maybe_responder() {
                    effects.extend(responder.respond(Err(Error::InvalidBlockchain)).ignore());
                }
                return effects;
            }
        };

        let prestate_hash = *block.state_root_hash();
        let account_hash = event_metadata.deploy().header().account().to_account_hash();
        let account_key = account_hash.into();

        effect_builder
            .get_account_from_global_state(prestate_hash, account_key)
            .event(move |maybe_account| Event::GetAccountResult {
                event_metadata,
                maybe_account,
                prestate_hash,
                verification_start_timestamp,
            })
    }

    fn handle_get_account_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        maybe_account: Option<Account>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        match maybe_account {
            None => {
                let account_hash = event_metadata.deploy().header().account().to_account_hash();
                effect_builder
                    .immediately()
                    .event(move |_| Event::InvalidDeployResult {
                        event_metadata,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure: DeployParameterFailure::NonexistentAccount { account_hash },
                        },
                        verification_start_timestamp,
                    })
            }
            Some(account) => {
                let authorization_keys = event_metadata
                    .deploy()
                    .approvals()
                    .iter()
                    .map(|approval| approval.signer().to_account_hash())
                    .collect();
                if !account.can_authorize(&authorization_keys) {
                    return effect_builder.immediately().event(move |_| {
                        Event::InvalidDeployResult {
                            event_metadata,
                            error: Error::InvalidDeployParameters {
                                prestate_hash,
                                failure: DeployParameterFailure::InvalidAssociatedKeys,
                            },
                            verification_start_timestamp,
                        }
                    });
                }
                if !account.can_deploy_with(&authorization_keys) {
                    return effect_builder.immediately().event(move |_| {
                        Event::InvalidDeployResult {
                            event_metadata,
                            error: Error::InvalidDeployParameters {
                                prestate_hash,
                                failure: DeployParameterFailure::InsufficientDeploySignatureWeight,
                            },
                            verification_start_timestamp,
                        }
                    });
                }
                return if event_metadata.source().from_client() {
                    effect_builder
                        .check_purse_balance(prestate_hash, account.main_purse())
                        .event(move |maybe_balance_value| Event::GetBalanceResult {
                            event_metadata,
                            prestate_hash,
                            maybe_balance_value,
                            verification_start_timestamp,
                        })
                } else {
                    effect_builder
                        .immediately()
                        .event(move |_| Event::VerifyPaymentLogic {
                            event_metadata,
                            prestate_hash,
                            verification_start_timestamp,
                        })
                };
            }
        }
    }

    fn handle_get_balance_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        maybe_balance_value: Option<U512>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        if !event_metadata.source().from_client() {
            // This would only happen due to programmer error and should crash the node.
            // Balance checks for deploys received by from a peer will cause the network
            // to stall.
            panic!("Balance checks for deploys received from peers should never occur.")
        }
        match maybe_balance_value {
            None => effect_builder
                .immediately()
                .event(move |_| Event::InvalidDeployResult {
                    event_metadata,
                    error: Error::InvalidDeployParameters {
                        prestate_hash,
                        failure: DeployParameterFailure::UnknownBalance,
                    },
                    verification_start_timestamp,
                }),
            Some(balance) => {
                let has_minimum_balance = balance >= *MAX_PAYMENT;
                if !has_minimum_balance {
                    effect_builder
                        .immediately()
                        .event(move |_| Event::InvalidDeployResult {
                            event_metadata,
                            error: Error::InvalidDeployParameters {
                                prestate_hash,
                                failure: DeployParameterFailure::InsufficientBalance,
                            },
                            verification_start_timestamp,
                        })
                } else {
                    effect_builder
                        .immediately()
                        .event(move |_| Event::VerifyPaymentLogic {
                            event_metadata,
                            prestate_hash,
                            verification_start_timestamp,
                        })
                }
            }
        }
    }

    fn verify_payment_logic<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let executable_deploy_item = event_metadata.deploy().payment();
        let mut maybe_failure: Option<DeployParameterFailure> = None;
        if let ExecutableDeployItem::Transfer { .. } = executable_deploy_item {
            maybe_failure = Some(DeployParameterFailure::InvalidPaymentVariant);
        }

        if let ExecutableDeployItem::ModuleBytes { module_bytes, args } = executable_deploy_item {
            if !module_bytes.is_empty() {
                if let Some(value) = args.get(ARG_AMOUNT) {
                    let _ = value.clone().into_t::<U512>().map_err(|_| {
                        maybe_failure = Some(DeployParameterFailure::FailedToParsePaymentAmount)
                    });
                } else {
                    maybe_failure = Some(DeployParameterFailure::MissingPaymentAmount)
                }
            }
        }

        match maybe_failure {
            Some(failure) => {
                effect_builder
                    .immediately()
                    .event(move |_| Event::InvalidDeployResult {
                        event_metadata,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure,
                        },
                        verification_start_timestamp,
                    })
            }
            None => {
                let executable_deploy_item_identifier = executable_deploy_item.identifier();
                let account_hash = event_metadata.deploy().header().account().to_account_hash();
                match executable_deploy_item_identifier {
                    ExecutableDeployItemIdentifier::Module
                    | ExecutableDeployItemIdentifier::Transfer => effect_builder
                        .immediately()
                        .event(move |_| Event::VerifySessionLogic {
                            event_metadata,
                            prestate_hash,
                            verification_start_timestamp,
                        }),
                    ExecutableDeployItemIdentifier::Contract(contract_identifier) => {
                        let (query_key, path) = match contract_identifier.clone() {
                            ContractIdentifier::Name(name) => {
                                let query_key: Key = account_hash.into();
                                let path = vec![name];
                                (query_key, path)
                            }
                            ContractIdentifier::Hash(contract_hash) => {
                                let query_key: Key = contract_hash.into();
                                (query_key, vec![])
                            }
                        };
                        effect_builder
                            .get_contract_for_validation(prestate_hash, query_key, path)
                            .event(move |maybe_contract| Event::GetContractResult {
                                event_metadata,
                                prestate_hash,
                                is_payment: true,
                                contract_identifier,
                                maybe_contract,
                                verification_start_timestamp,
                            })
                    }
                    ExecutableDeployItemIdentifier::Package(contract_package_identifier) => {
                        let (query_key, path): (Key, Vec<String>) =
                            match contract_package_identifier.clone() {
                                ContractPackageIdentifier::Name { name, .. } => {
                                    let query_key: Key = account_hash.into();
                                    let path = vec![name];
                                    (query_key, path)
                                }
                                ContractPackageIdentifier::Hash {
                                    contract_package_hash,
                                    ..
                                } => {
                                    let query_key = contract_package_hash.into();
                                    (query_key, vec![])
                                }
                            };
                        effect_builder
                            .get_contract_package_for_validation(prestate_hash, query_key, path)
                            .event(
                                move |maybe_contract_package| Event::GetContractPackageResult {
                                    event_metadata,
                                    prestate_hash,
                                    is_payment: true,
                                    contract_package_identifier,
                                    maybe_contract_package,
                                    verification_start_timestamp,
                                },
                            )
                    }
                }
            }
        }
    }

    fn verify_session_logic<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let executable_deploy_item = event_metadata.deploy().session();
        let mut maybe_failure: Option<DeployParameterFailure> = None;
        if let ExecutableDeployItem::Transfer { args } = executable_deploy_item {
            if let Some(value) = args.get(ARG_AMOUNT) {
                let _ = value.clone().into_t::<U512>().map_err(|_| {
                    maybe_failure = Some(DeployParameterFailure::FailedToParseTransferAmount)
                });
            } else {
                maybe_failure = Some(DeployParameterFailure::MissingTransferAmount)
            }
            if args.get("target").is_none() {
                maybe_failure = Some(DeployParameterFailure::MissingTransferTargetArgument)
            }
            if args.get("source").is_none() {
                maybe_failure = Some(DeployParameterFailure::MissingTransferSourceArgument)
            }
        }

        if let ExecutableDeployItem::ModuleBytes { module_bytes, .. } = executable_deploy_item {
            if module_bytes.is_empty() {
                maybe_failure = Some(DeployParameterFailure::MissingModuleBytes)
            }
        }

        match maybe_failure {
            Some(failure) => {
                effect_builder
                    .immediately()
                    .event(move |_| Event::InvalidDeployResult {
                        event_metadata,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure,
                        },
                        verification_start_timestamp,
                    })
            }
            None => {
                let executable_deploy_item_identifier = executable_deploy_item.identifier();
                let account_hash = event_metadata.deploy().header().account().to_account_hash();
                match executable_deploy_item_identifier {
                    ExecutableDeployItemIdentifier::Module
                    | ExecutableDeployItemIdentifier::Transfer => effect_builder
                        .immediately()
                        .event(move |_| Event::VerifyDeployCryptographicValidity {
                            event_metadata,
                            verification_start_timestamp,
                        }),
                    ExecutableDeployItemIdentifier::Contract(contract_identifier) => {
                        let (query_key, path) = match contract_identifier.clone() {
                            ContractIdentifier::Name(name) => {
                                let query_key: Key = account_hash.into();
                                let path = vec![name];
                                (query_key, path)
                            }
                            ContractIdentifier::Hash(contract_hash) => {
                                let query_key: Key = contract_hash.into();
                                (query_key, vec![])
                            }
                        };
                        effect_builder
                            .get_contract_for_validation(prestate_hash, query_key, path)
                            .event(move |maybe_contract| Event::GetContractResult {
                                event_metadata,
                                prestate_hash,
                                is_payment: false,
                                contract_identifier,
                                maybe_contract,
                                verification_start_timestamp,
                            })
                    }
                    ExecutableDeployItemIdentifier::Package(contract_package_identifier) => {
                        let (query_key, path): (Key, Vec<String>) =
                            match contract_package_identifier.clone() {
                                ContractPackageIdentifier::Name { name, .. } => {
                                    let query_key: Key = account_hash.into();
                                    let path = vec![name];
                                    (query_key, path)
                                }
                                ContractPackageIdentifier::Hash {
                                    contract_package_hash,
                                    ..
                                } => {
                                    let query_key = contract_package_hash.into();
                                    (query_key, vec![])
                                }
                            };
                        effect_builder
                            .get_contract_package_for_validation(prestate_hash, query_key, path)
                            .event(
                                move |maybe_contract_package| Event::GetContractPackageResult {
                                    event_metadata,
                                    prestate_hash,
                                    is_payment: false,
                                    contract_package_identifier,
                                    maybe_contract_package,
                                    verification_start_timestamp,
                                },
                            )
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_get_contract_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        is_payment: bool,
        entry_point: String,
        contract_identifier: ContractIdentifier,
        maybe_contract: Option<Contract>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        if let Some(contract) = maybe_contract {
            if !contract.entry_points().has_entry_point(&entry_point) {
                let failure = DeployParameterFailure::NonexistentContractEntryPoint { entry_point };
                return effect_builder
                    .immediately()
                    .event(move |_| Event::InvalidDeployResult {
                        event_metadata,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure,
                        },
                        verification_start_timestamp,
                    });
            }
            return if is_payment {
                effect_builder
                    .immediately()
                    .event(move |_| Event::VerifySessionLogic {
                        event_metadata,
                        prestate_hash,
                        verification_start_timestamp,
                    })
            } else {
                effect_builder.immediately().event(move |_| {
                    Event::VerifyDeployCryptographicValidity {
                        event_metadata,
                        verification_start_timestamp,
                    }
                })
            };
        }

        let failure = match contract_identifier {
            ContractIdentifier::Name(name) => {
                DeployParameterFailure::NonexistentContractAtName { name }
            }
            ContractIdentifier::Hash(contract_hash) => {
                DeployParameterFailure::NonexistentContractAtHash { contract_hash }
            }
        };

        effect_builder
            .immediately()
            .event(move |_| Event::InvalidDeployResult {
                event_metadata,
                error: Error::InvalidDeployParameters {
                    prestate_hash,
                    failure,
                },
                verification_start_timestamp,
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_get_contract_package_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        is_payment: bool,
        contract_package_identifier: ContractPackageIdentifier,
        maybe_contract_package: Option<ContractPackage>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        match maybe_contract_package {
            None => {
                let failure = match contract_package_identifier {
                    ContractPackageIdentifier::Name { name, .. } => {
                        DeployParameterFailure::NonexistentContractPackageAtName { name }
                    }
                    ContractPackageIdentifier::Hash {
                        contract_package_hash,
                        ..
                    } => DeployParameterFailure::NonexistentContractPackageAtHash {
                        contract_package_hash,
                    },
                };

                effect_builder
                    .immediately()
                    .event(move |_| Event::InvalidDeployResult {
                        event_metadata,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure,
                        },
                        verification_start_timestamp,
                    })
            }
            Some(contract_package) => match contract_package_identifier.version() {
                Some(contract_version) => {
                    let contract_version_key = ContractVersionKey::new(
                        self.protocol_version.value().major,
                        contract_version,
                    );
                    match contract_package.lookup_contract_hash(contract_version_key) {
                        Some(&contract_hash) => {
                            let query_key = contract_hash.into();
                            effect_builder
                                .get_contract_for_validation(prestate_hash, query_key, vec![])
                                .event(move |maybe_contract| Event::GetContractResult {
                                    event_metadata,
                                    prestate_hash,
                                    is_payment,
                                    contract_identifier: ContractIdentifier::Hash(contract_hash),
                                    maybe_contract,
                                    verification_start_timestamp,
                                })
                        }
                        None => effect_builder.immediately().event(move |_| {
                            Event::InvalidDeployResult {
                                event_metadata,
                                error: Error::InvalidDeployParameters {
                                    prestate_hash,
                                    failure: DeployParameterFailure::InvalidContractAtVersion {
                                        contract_version,
                                    },
                                },
                                verification_start_timestamp,
                            }
                        }),
                    }
                }
                None => {
                    if is_payment {
                        effect_builder
                            .immediately()
                            .event(move |_| Event::VerifySessionLogic {
                                event_metadata,
                                prestate_hash,
                                verification_start_timestamp,
                            })
                    } else {
                        effect_builder.immediately().event(move |_| {
                            Event::VerifyDeployCryptographicValidity {
                                event_metadata,
                                verification_start_timestamp,
                            }
                        })
                    }
                }
            },
        }
    }

    fn handle_put_to_storage<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        is_new: bool,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let EventMetadata {
            deploy,
            source,
            maybe_responder,
        } = event_metadata;
        self.metrics.observe_accepted(verification_start_timestamp);
        let mut effects = Effects::new();
        if is_new {
            effects.extend(
                effect_builder
                    .announce_new_deploy_accepted(deploy, source)
                    .ignore(),
            );
        }

        // success
        if let Some(responder) = maybe_responder {
            effects.extend(responder.respond(Ok(())).ignore());
        }
        effects
    }

    fn validate_deploy_cryptography<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let mut cloned_deploy = event_metadata.deploy().clone();
        if let Err(deploy_configuration_failure) = cloned_deploy.is_valid() {
            // The client has submitted a deploy with one or more invalid signatures.
            // Return an error to the RPC component via the responder.
            return effect_builder
                .immediately()
                .event(move |_| Event::InvalidDeployResult {
                    event_metadata,
                    error: Error::InvalidDeployConfiguration(deploy_configuration_failure),
                    verification_start_timestamp,
                });
        }

        effect_builder
            .put_deploy_to_storage(Box::new(event_metadata.deploy().clone()))
            .event(move |is_new| Event::PutToStorageResult {
                event_metadata,
                is_new,
                verification_start_timestamp,
            })
    }
}

impl<REv: ReactorEventT> Component<REv> for DeployAcceptor {
    type Event = Event;
    type ConstructionError = prometheus::Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Accept {
                deploy,
                source,
                maybe_responder: responder,
            } => self.accept(effect_builder, deploy, source, responder),
            Event::GetBlockResult {
                event_metadata,
                maybe_block,
                verification_start_timestamp,
            } => self.handle_get_block_result(
                effect_builder,
                event_metadata,
                *maybe_block,
                verification_start_timestamp,
            ),
            Event::GetAccountResult {
                event_metadata,
                prestate_hash,
                maybe_account,
                verification_start_timestamp,
            } => self.handle_get_account_result(
                effect_builder,
                event_metadata,
                prestate_hash,
                maybe_account,
                verification_start_timestamp,
            ),
            Event::GetBalanceResult {
                event_metadata,
                prestate_hash,
                maybe_balance_value,
                verification_start_timestamp,
            } => self.handle_get_balance_result(
                effect_builder,
                event_metadata,
                prestate_hash,
                maybe_balance_value,
                verification_start_timestamp,
            ),
            Event::VerifyPaymentLogic {
                event_metadata,
                prestate_hash,
                verification_start_timestamp,
            } => self.verify_payment_logic(
                effect_builder,
                event_metadata,
                prestate_hash,
                verification_start_timestamp,
            ),
            Event::VerifySessionLogic {
                event_metadata,
                prestate_hash,
                verification_start_timestamp,
            } => self.verify_session_logic(
                effect_builder,
                event_metadata,
                prestate_hash,
                verification_start_timestamp,
            ),
            Event::GetContractResult {
                event_metadata,
                prestate_hash,
                is_payment,
                contract_identifier,
                maybe_contract,
                verification_start_timestamp,
            } => {
                let entry_point = if is_payment {
                    event_metadata.deploy().payment().entry_point_name()
                } else {
                    event_metadata.deploy().session().entry_point_name()
                }
                .to_string();
                self.handle_get_contract_result(
                    effect_builder,
                    event_metadata,
                    prestate_hash,
                    is_payment,
                    entry_point,
                    contract_identifier,
                    maybe_contract,
                    verification_start_timestamp,
                )
            }
            Event::GetContractPackageResult {
                event_metadata,
                prestate_hash,
                is_payment,
                contract_package_identifier,
                maybe_contract_package,
                verification_start_timestamp,
            } => self.handle_get_contract_package_result(
                effect_builder,
                event_metadata,
                prestate_hash,
                is_payment,
                contract_package_identifier,
                maybe_contract_package,
                verification_start_timestamp,
            ),
            Event::VerifyDeployCryptographicValidity {
                event_metadata,
                verification_start_timestamp,
            } => self.validate_deploy_cryptography(
                effect_builder,
                event_metadata,
                verification_start_timestamp,
            ),
            Event::PutToStorageResult {
                event_metadata,
                is_new,
                verification_start_timestamp,
            } => self.handle_put_to_storage(
                effect_builder,
                event_metadata,
                is_new,
                verification_start_timestamp,
            ),
            Event::InvalidDeployResult {
                event_metadata,
                error,
                verification_start_timestamp,
            } => {
                let EventMetadata {
                    deploy,
                    source,
                    maybe_responder,
                } = event_metadata;
                self.metrics.observe_rejected(verification_start_timestamp);
                let mut effects = Effects::new();
                // The client has submitted a deploy with one or more invalid signatures.
                // Return an error to the RPC component via the responder.
                if let Some(responder) = maybe_responder {
                    effects.extend(responder.respond(Err(error)).ignore());
                }
                effects.extend(
                    effect_builder
                        .announce_invalid_deploy(deploy, source)
                        .ignore(),
                );
                effects
            }
        }
    }
}
