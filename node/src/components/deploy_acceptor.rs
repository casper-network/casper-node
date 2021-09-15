mod config;
mod event;
mod tests;

use std::{convert::Infallible, fmt::Debug};

use datasize::DataSize;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error};

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
    },
    utils::Source,
    NodeRng,
};

use casper_execution_engine::{
    core::engine_state::{
        executable_deploy_item::{
            ContractIdentifier, ContractPackageIdentifier, ExecutableDeployItemIdentifier,
        },
        ExecutableDeployItem, MAX_PAYMENT,
    },
    shared::newtypes::Blake2bHash,
};

pub(crate) use config::Config;
pub(crate) use event::Event;

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
        prestate_hash: Blake2bHash,
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
}

impl DeployAcceptor {
    pub(crate) fn new(config: Config, chainspec: &Chainspec) -> Self {
        DeployAcceptor {
            chain_name: chainspec.network_config.name.clone(),
            protocol_version: chainspec.protocol_version(),
            deploy_config: chainspec.deploy_config,
            verify_accounts: config.verify_accounts(),
        }
    }

    fn accept<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let mut cloned_deploy = deploy.clone();
        let acceptable_result =
            cloned_deploy.is_config_compliant(&self.chain_name, &self.deploy_config);
        // checks chainspec values
        // DOES NOT check cryptographic security
        if let Err(error) = acceptable_result {
            return effect_builder
                .immediately()
                .event(move |_| Event::InvalidDeployResult {
                    deploy,
                    source,
                    error: Error::InvalidDeployConfiguration(error),
                    maybe_responder,
                });
        }

        effect_builder
            .get_highest_block_from_storage()
            .event(move |maybe_block| Event::GetBlockResult {
                deploy,
                source,
                maybe_block: Box::new(maybe_block),
                maybe_responder,
            })
    }

    fn handle_get_block_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_block: Option<Block>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let mut effects = Effects::new();

        let block = match maybe_block {
            Some(block) => block,
            None => {
                // this should be unreachable per current design of the system
                if let Some(responder) = maybe_responder {
                    effects.extend(responder.respond(Err(Error::InvalidBlockchain)).ignore());
                }
                return effects;
            }
        };

        let prestate_hash = (*block.state_root_hash()).into();
        let account_hash = deploy.header().account().to_account_hash();
        let account_key = account_hash.into();

        effect_builder
            .get_account_from_global_state(prestate_hash, account_key)
            .event(move |maybe_account| Event::GetAccountResult {
                deploy,
                source,
                maybe_account,
                prestate_hash,
                maybe_responder,
            })
    }

    fn handle_get_account_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        maybe_account: Option<Account>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        match maybe_account {
            None => {
                let account_hash = deploy.header().account().to_account_hash();
                effect_builder
                    .immediately()
                    .event(move |_| Event::InvalidDeployResult {
                        deploy,
                        source,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure: DeployParameterFailure::NonexistentAccount { account_hash },
                        },
                        maybe_responder,
                    })
            }
            Some(account) => {
                let authorization_keys = deploy
                    .approvals()
                    .iter()
                    .map(|approval| approval.signer().to_account_hash())
                    .collect();
                if !account.can_authorize(&authorization_keys) {
                    return effect_builder.immediately().event(move |_| {
                        Event::InvalidDeployResult {
                            deploy,
                            source,
                            error: Error::InvalidDeployParameters {
                                prestate_hash,
                                failure: DeployParameterFailure::InvalidAssociatedKeys,
                            },
                            maybe_responder,
                        }
                    });
                }
                if !account.can_deploy_with(&authorization_keys) {
                    return effect_builder.immediately().event(move |_| {
                        Event::InvalidDeployResult {
                            deploy,
                            source,
                            error: Error::InvalidDeployParameters {
                                prestate_hash,
                                failure: DeployParameterFailure::InsufficientDeploySignatureWeight,
                            },
                            maybe_responder,
                        }
                    });
                }
                return if source.from_client() {
                    effect_builder
                        .check_purse_balance(prestate_hash, account.main_purse())
                        .event(move |maybe_balance_value| Event::GetBalanceResult {
                            deploy,
                            source,
                            prestate_hash,
                            maybe_balance_value,
                            maybe_responder,
                        })
                } else {
                    effect_builder
                        .immediately()
                        .event(move |_| Event::VerifyPaymentLogic {
                            deploy,
                            source,
                            prestate_hash,
                            maybe_responder,
                        })
                };
            }
        }
    }

    fn handle_get_balance_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        maybe_balance_value: Option<U512>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        if !source.from_client() {
            // This would only happen due to programmer error and should crash the node.
            // Balance checks for deploys received by from a peer will cause the network
            // to stall.
            panic!("Balance checks for deploys received from peers should never occur.")
        }
        match maybe_balance_value {
            None => effect_builder
                .immediately()
                .event(move |_| Event::InvalidDeployResult {
                    deploy,
                    source,
                    error: Error::InvalidDeployParameters {
                        prestate_hash,
                        failure: DeployParameterFailure::UnknownBalance,
                    },
                    maybe_responder,
                }),
            Some(balance) => {
                let has_minimum_balance = balance >= *MAX_PAYMENT;
                if !has_minimum_balance {
                    effect_builder
                        .immediately()
                        .event(move |_| Event::InvalidDeployResult {
                            deploy,
                            source,
                            error: Error::InvalidDeployParameters {
                                prestate_hash,
                                failure: DeployParameterFailure::InsufficientBalance,
                            },
                            maybe_responder,
                        })
                } else {
                    effect_builder
                        .immediately()
                        .event(move |_| Event::VerifyPaymentLogic {
                            deploy,
                            source,
                            prestate_hash,
                            maybe_responder,
                        })
                }
            }
        }
    }

    fn verify_payment_logic<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let executable_deploy_item = deploy.payment();
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
                        deploy,
                        source,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure,
                        },
                        maybe_responder,
                    })
            }
            None => {
                let executable_deploy_item_identifier = executable_deploy_item.identifier();
                let account_hash = deploy.header().account().to_account_hash();
                match executable_deploy_item_identifier {
                    ExecutableDeployItemIdentifier::Module
                    | ExecutableDeployItemIdentifier::Transfer => effect_builder
                        .immediately()
                        .event(move |_| Event::VerifySessionLogic {
                            deploy,
                            source,
                            prestate_hash,
                            maybe_responder,
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
                                deploy,
                                source,
                                prestate_hash,
                                is_payment: true,
                                contract_identifier,
                                maybe_contract,
                                maybe_responder,
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
                                    deploy,
                                    source,
                                    prestate_hash,
                                    is_payment: true,
                                    contract_package_identifier,
                                    maybe_contract_package,
                                    maybe_responder,
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
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let executable_deploy_item = deploy.session();
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
                        deploy,
                        source,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure,
                        },
                        maybe_responder,
                    })
            }
            None => {
                let executable_deploy_item_identifier = executable_deploy_item.identifier();
                let account_hash = deploy.header().account().to_account_hash();
                match executable_deploy_item_identifier {
                    ExecutableDeployItemIdentifier::Module
                    | ExecutableDeployItemIdentifier::Transfer => effect_builder
                        .immediately()
                        .event(move |_| Event::VerifyDeployCryptographicValidity {
                            deploy,
                            source,
                            maybe_responder,
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
                                deploy,
                                source,
                                prestate_hash,
                                is_payment: false,
                                contract_identifier,
                                maybe_contract,
                                maybe_responder,
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
                                    deploy,
                                    source,
                                    prestate_hash,
                                    is_payment: false,
                                    contract_package_identifier,
                                    maybe_contract_package,
                                    maybe_responder,
                                },
                            )
                    }
                }
            }
        }
    }

    fn _get_executable_item_query_info(
        &self,
        account_hash: AccountHash,
        executable_deploy_item_identifier: ExecutableDeployItemIdentifier,
    ) -> Option<(Key, Vec<String>)> {
        match executable_deploy_item_identifier {
            ExecutableDeployItemIdentifier::Module | ExecutableDeployItemIdentifier::Transfer => {
                None
            }
            ExecutableDeployItemIdentifier::Contract(contract_identifier) => {
                match contract_identifier {
                    ContractIdentifier::Name(name) => {
                        let key: Key = account_hash.into();
                        Some((key, vec![name]))
                    }
                    ContractIdentifier::Hash(contract_hash) => Some((contract_hash.into(), vec![])),
                }
            }
            ExecutableDeployItemIdentifier::Package(contract_package_identifier) => {
                match contract_package_identifier {
                    ContractPackageIdentifier::Name { name, .. } => {
                        let key: Key = account_hash.into();
                        Some((key, vec![name]))
                    }
                    ContractPackageIdentifier::Hash {
                        contract_package_hash,
                        ..
                    } => Some((contract_package_hash.into(), vec![])),
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_get_contract_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        is_payment: bool,
        entry_point: String,
        contract_identifier: ContractIdentifier,
        maybe_contract: Option<Contract>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        if let Some(contract) = maybe_contract {
            if !contract.entry_points().has_entry_point(&entry_point) {
                let failure = DeployParameterFailure::NonexistentContractEntryPoint { entry_point };
                return effect_builder
                    .immediately()
                    .event(move |_| Event::InvalidDeployResult {
                        deploy,
                        source,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure,
                        },
                        maybe_responder,
                    });
            }
            return if is_payment {
                effect_builder
                    .immediately()
                    .event(move |_| Event::VerifySessionLogic {
                        deploy,
                        source,
                        prestate_hash,
                        maybe_responder,
                    })
            } else {
                effect_builder.immediately().event(move |_| {
                    Event::VerifyDeployCryptographicValidity {
                        deploy,
                        source,
                        maybe_responder,
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
                deploy,
                source,
                error: Error::InvalidDeployParameters {
                    prestate_hash,
                    failure,
                },
                maybe_responder,
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_get_contract_package_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        is_payment: bool,
        contract_package_identifier: ContractPackageIdentifier,
        maybe_contract_package: Option<ContractPackage>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
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
                        deploy,
                        source,
                        error: Error::InvalidDeployParameters {
                            prestate_hash,
                            failure,
                        },
                        maybe_responder,
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
                                    deploy,
                                    source,
                                    prestate_hash,
                                    is_payment,
                                    contract_identifier: ContractIdentifier::Hash(contract_hash),
                                    maybe_contract,
                                    maybe_responder,
                                })
                        }
                        None => effect_builder.immediately().event(move |_| {
                            Event::InvalidDeployResult {
                                deploy,
                                source,
                                error: Error::InvalidDeployParameters {
                                    prestate_hash,
                                    failure: DeployParameterFailure::InvalidContractAtVersion {
                                        contract_version,
                                    },
                                },
                                maybe_responder,
                            }
                        }),
                    }
                }
                None => {
                    if is_payment {
                        effect_builder
                            .immediately()
                            .event(move |_| Event::VerifySessionLogic {
                                deploy,
                                source,
                                prestate_hash,
                                maybe_responder,
                            })
                    } else {
                        effect_builder.immediately().event(move |_| {
                            Event::VerifyDeployCryptographicValidity {
                                deploy,
                                source,
                                maybe_responder,
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
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        is_new: bool,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
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

    // /// Handles receiving a new `Deploy` from a peer or client.
    // /// In the case of a peer, there should be no responder and the variant should be `None`
    // /// In the case of a client, there should be a responder to communicate the validity of the
    // /// deploy and the variant will be `Some`
    // fn accept<REv: ReactorEventT>(
    //     &mut self,
    //     effect_builder: EffectBuilder<REv>,
    //     deploy: Box<Deploy>,
    //     source: Source<NodeId>,
    //     maybe_responder: Option<Responder<Result<(), Error>>>,
    // ) -> Effects<Event> {
    //     let mut effects = Effects::new();
    //     let mut cloned_deploy = deploy.clone();
    //     let acceptable_result =
    //         cloned_deploy.is_config_compliant(&self.chain_name, &self.deploy_config);
    //     // checks chainspec values
    //     // DOES NOT check cryptographic security
    //     if let Err(error) = acceptable_result {
    //         return effect_builder
    //             .immediately()
    //             .event(move |_| Event::InvalidDeployResult {
    //                 deploy,
    //                 source,
    //                 error: Error::InvalidDeployConfiguration(error),
    //                 maybe_responder,
    //             });
    //     }
    //
    //     // Event::GetHighestBlock
    //     let block = {
    //         match effect_builder.get_highest_block_from_storage().await {
    //             Some(block) => block,
    //             None => {
    //                 // this should be unreachable per current design of the system
    //                 if let Some(responder) = maybe_responder {
    //
    // effects.extend(responder.respond(Err(Error::InvalidBlockchain)).ignore());
    // }                 return effects;
    //             }
    //         }
    //     };
    //
    //     let prestate_hash = block.state_root_hash().into();
    //
    //     let valid_account_result =
    //         self.is_valid_account(effect_builder, prestate_hash, cloned_deploy);
    //     // many EE preconditions
    //     // DOES NOT check cryptographic security
    //     if let Err(failure) = valid_account_result {
    //         return effect_builder
    //             .immediately()
    //             .event(move |_| Event::InvalidDeployResult {
    //                 deploy,
    //                 source,
    //                 error: Error::InvalidDeployParameters {
    //                     prestate_hash,
    //                     failure,
    //                 },
    //                 maybe_responder,
    //             });
    //     }
    //
    //     if let Err(failure) =
    //         self.is_valid_executable_deploy_item(effect_builder, prestate_hash, deploy.payment())
    //     {
    //         return effect_builder
    //             .immediately()
    //             .event(move |_| Event::InvalidDeployResult {
    //                 deploy,
    //                 source,
    //                 error: Error::InvalidDeployParameters {
    //                     prestate_hash,
    //                     failure,
    //                 },
    //                 maybe_responder,
    //             });
    //     }
    //
    //     if let Err(failure) =
    //         self.is_valid_executable_deploy_item(effect_builder, prestate_hash, deploy.session())
    //     {
    //         return effect_builder
    //             .immediately()
    //             .event(move |_| Event::InvalidDeployResult {
    //                 deploy,
    //                 source,
    //                 error: Error::InvalidDeployParameters {
    //                     prestate_hash: prestate_hash,
    //                     failure,
    //                 },
    //                 maybe_responder,
    //             });
    //     }
    //
    //     let account_key = cloned_deploy.header().account().to_account_hash().into();
    //
    //     // if received from client, check to see if account exists and has at least
    //     // enough motes to cover the penalty payment (aka has at least the minimum account
    // balance)     // NEVER CHECK THIS for deploys received from other nodes
    //     let verified_account = {
    //         if source.from_client() {
    //             effect_builder.is_verified_account(account_key).await
    //         } else {
    //             Some(true)
    //         }
    //     };
    //
    //     match verified_account {
    //         None => {
    //             // The client has submitted an invalid deploy.
    //             // Return an error message to the RPC component via the responder.
    //             info! {
    //                 "Received deploy from invalid account using {}", account_key
    //             };
    //             if let Some(responder) = maybe_responder {
    //                 effects.extend(responder.respond(Err(Error::InvalidAccount)).ignore());
    //             }
    //             return effects;
    //         }
    //
    //         Some(false) => {
    //             info! {
    //                 "Received deploy from account {} that does not have minimum balance
    // required", account_key             };
    //             // The client has submitted a deploy from an account that does not have minimum
    //             // balance required. Return an error message to the RPC component via the
    // responder.             if let Some(responder) = maybe_responder {
    //                 effects.extend(responder.respond(Err(Error::InsufficientBalance)).ignore());
    //             }
    //             return effects;
    //         }
    //
    //         Some(true) => {
    //             // noop; can proceed
    //         }
    //     }
    //
    //     // check cryptographic signature(s) on the deploy
    //     // do this last as this is computationally expensive and there is
    //     // no reason to do it if the deploy is invalid
    //     if let Err(deploy_configuration_failure) = cloned_deploy.is_valid() {
    //         // The client has submitted a deploy with one or more invalid signatures.
    //         // Return an error to the RPC component via the responder.
    //         return effect_builder
    //             .immediately()
    //             .event(move |_| Event::InvalidDeployResult {
    //                 deploy,
    //                 source,
    //                 error: Error::InvalidDeployConfiguration(deploy_configuration_failure),
    //                 maybe_responder,
    //             });
    //     }
    //
    //     let is_new = effect_builder.put_deploy_to_storage(deploy.clone()).await;
    //     if is_new {
    //         effects.extend(
    //             effect_builder
    //                 .announce_new_deploy_accepted(deploy, source.clone())
    //                 .ignore(),
    //         );
    //     }
    //
    //     // success
    //     if let Some(responder) = maybe_responder {
    //         effects.extend(responder.respond(Ok(())).ignore());
    //     }
    //     effects
    // }

    fn validate_deploy_cryptography<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let mut cloned_deploy = deploy.clone();
        if let Err(deploy_configuration_failure) = cloned_deploy.is_valid() {
            // The client has submitted a deploy with one or more invalid signatures.
            // Return an error to the RPC component via the responder.
            return effect_builder
                .immediately()
                .event(move |_| Event::InvalidDeployResult {
                    deploy,
                    source,
                    error: Error::InvalidDeployConfiguration(deploy_configuration_failure),
                    maybe_responder,
                });
        }
        effect_builder
            .put_deploy_to_storage(deploy.clone())
            .event(move |is_new| Event::PutToStorageResult {
                deploy,
                source,
                is_new,
                maybe_responder,
            })
    }

    // fn foo_is_valid_account(
    //     &self,
    //     deploy: Box<Deploy>,
    //     account_hash: AccountHash,
    //     query_result: Result<QueryResult, EngineError>,
    // ) -> Result<Account, DeployParameterFailure> {
    //     match query_result {
    //         Ok(QueryResult::Success { value, .. }) => {
    //             if let StoredValue::Account(account) = *value {
    //                 // check account here
    //                 let authorization_keys = deploy
    //                     .approvals()
    //                     .iter()
    //                     .map(|approval| approval.signer().to_account_hash())
    //                     .collect();
    //                 if account.can_authorize(&authorization_keys) == false {
    //                     return Err(DeployParameterFailure::InvalidAssociatedKeys);
    //                 }
    //                 if account.can_deploy_with(&authorization_keys) == false {
    //                     return Err(DeployParameterFailure::InsufficientDeploySignatureWeight);
    //                 }
    //                 // We have confirmed the account's validity.
    //                 // We can separately check for the account balance.
    //                 return Ok(account);
    //             }
    //             Err(DeployParameterFailure::NonexistentAccount { account_hash })
    //         }
    //         Ok(QueryResult::RootNotFound) => Err(DeployParameterFailure::InvalidGlobalStateHash),
    //         Ok(query_result) => Err(DeployParameterFailure::InvalidQuery {
    //             key: account_hash.into(),
    //             message: format!("{:?}", query_result),
    //         }),
    //         Err(error) => Err(DeployParameterFailure::InvalidQuery {
    //             key: account_hash.into(),
    //             message: error.to_string(),
    //         }),
    //     }
    // }

    // fn is_valid_executable_deploy_item<REv: ReactorEventT>(
    //     &self,
    //     effect_builder: EffectBuilder<REv>,
    //     prestate_hash: Blake2bHash,
    //     executable_deploy_item: &ExecutableDeployItem,
    //     account_hash: AccountHash,
    // ) -> Effects<Event> {
    //     match executable_deploy_item.identifier() {
    //         ExecutableDeployItemIdentifier::Module | ExecutableDeployItemIdentifier::Transfer =>
    // {             Effects::new()
    //         }
    //         ExecutableDeployItemIdentifier::Contract(contract_identifier) => {
    //             let query_request = match contract_identifier.clone() {
    //                 ContractIdentifier::Name(name) => {
    //                     let query_key = account_hash.into();
    //                     let path = vec![name];
    //                     QueryRequest::new(prestate_hash, query_key, path)
    //                 }
    //                 ContractIdentifier::Hash(contract_hash) => {
    //                     let query_key = contract_hash.into();
    //                     let path = vec![];
    //                     QueryRequest::new(prestate_hash, query_key, path)
    //                 }
    //             };
    //             let entry_point = executable_deploy_item.entry_point_name().to_string();
    //         }
    //         ExecutableDeployItemIdentifier::Package(contract_package_identifier) => {
    //             let query_request = match contract_package_identifier.clone() {
    //                 ContractIdentifier::Name(name) => {
    //                     let account_hash = deploy.header().account().to_account_hash();
    //                     let query_key = account_hash.into();
    //                     let path = vec![name];
    //                     QueryRequest::new(prestate_hash, query_key, path)
    //                 }
    //                 ContractIdentifier::Hash(contract_hash) => {
    //                     let query_key = contract_hash.into();
    //                     let path = vec![];
    //                     QueryRequest::new(prestate_hash, query_key, path)
    //                 }
    //             };
    //             effect_builder
    //                 .query_global_state(query_request)
    //                 .event(move |query_result| Event::ValidateContractPackage {
    //                     entry_point,
    //                     query_key,
    //                     query_result,
    //                     contract_package_identifier,
    //                 })
    //         }
    //     }
    // }

    // fn is_valid_contract(
    //     &self,
    //     entry_point: String,
    //     query_key: Key,
    //     contract_identifier: ContractIdentifier,
    //     query_result: Result<QueryResult, EngineError>,
    // ) -> Result<(), DeployParameterFailure> {
    //     match query_result {
    //         Ok(QueryResult::Success { value, .. }) => {
    //             if let StoredValue::Contract(contract) = *value {
    //                 if contract.entry_points().has_entry_point(&entry_point) == false {
    //                     return Err(DeployParameterFailure::NonexistentContractEntryPoint {
    //                         entry_point,
    //                     });
    //                 }
    //                 return Ok(());
    //             }
    //             match contract_identifier {
    //                 ContractIdentifier::Name(name) => {
    //                     return Err(DeployParameterFailure::NonexistentContractAtName { name });
    //                 }
    //                 ContractIdentifier::Hash(contract_hash) => {
    //                     return Err(DeployParameterFailure::NonexistentContractAtHash {
    //                         contract_hash,
    //                     });
    //                 }
    //             }
    //         }
    //         Ok(QueryResult::RootNotFound) => {
    //             return Err(DeployParameterFailure::InvalidGlobalStateHash);
    //         }
    //         Ok(query_result) => {
    //             return Err(DeployParameterFailure::InvalidQuery {
    //                 key: query_key,
    //                 message: format!("{:?}", query_result),
    //             });
    //         }
    //         Err(error) => {
    //             return Err(DeployParameterFailure::InvalidQuery {
    //                 key: query_key,
    //                 message: error.to_string(),
    //             });
    //         }
    //     }
    // }

    // fn is_valid_contract_package(
    //     &self,
    //     entry_point: String,
    //     query_key: Key,
    //     query_result: Result<QueryResult, EngineError>,
    //     contract_package_identifier: ContractPackageIdentifier,
    // ) -> Result<(), DeployParameterFailure> {
    //     match query_result {
    //         Ok(QueryResult::Success { value, .. }) => {
    //             if let StoredValue::ContractPackage(contract_package) = *value {
    //                 // get the contract version of the package, then get the contract, then
    //                 // call is_valid_contract
    //
    //                 if contract_package
    //                     .entry_points()
    //                     .has_entry_point(&entry_point)
    //                     == false
    //                 {
    //                     return Err(
    //                         DeployParameterFailure::NonexistentContractPackageEntryPoint {
    //                             entry_point,
    //                         },
    //                     );
    //                 }
    //                 return Ok(());
    //             }
    //             match contract_package_identifier {
    //                 ContractPackageIdentifier::Name(name) => {
    //                     return Err(DeployParameterFailure::NonexistentContractPackageAtName {
    //                         name,
    //                     });
    //                 }
    //                 ContractPackageIdentifier::Hash(contract_package_hash) => {
    //                     return Err(DeployParameterFailure::NonexistentContractPackageAtHash {
    //                         contract_package_hash,
    //                     });
    //                 }
    //             }
    //         }
    //         Ok(QueryResult::RootNotFound) => {
    //             return Err(DeployParameterFailure::InvalidGlobalStateHash);
    //         }
    //         Ok(query_result) => {
    //             return Err(DeployParameterFailure::InvalidQuery {
    //                 key: query_key,
    //                 message: format!("{:?}", query_result),
    //             });
    //         }
    //         Err(error) => {
    //             return Err(DeployParameterFailure::InvalidQuery {
    //                 key: query_key,
    //                 message: error.to_string(),
    //             });
    //         }
    //     }
    // }
}

impl<REv: ReactorEventT> Component<REv> for DeployAcceptor {
    type Event = Event;
    type ConstructionError = Infallible;

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
                responder,
            } => self.accept(effect_builder, deploy, source, responder),
            Event::GetBlockResult {
                deploy,
                source,
                maybe_block,
                maybe_responder,
            } => self.handle_get_block_result(
                effect_builder,
                deploy,
                source,
                *maybe_block,
                maybe_responder,
            ),
            Event::GetAccountResult {
                deploy,
                source,
                prestate_hash,
                maybe_account,
                maybe_responder,
            } => self.handle_get_account_result(
                effect_builder,
                deploy,
                source,
                prestate_hash,
                maybe_account,
                maybe_responder,
            ),
            Event::GetBalanceResult {
                deploy,
                source,
                prestate_hash,
                maybe_balance_value,
                maybe_responder,
            } => self.handle_get_balance_result(
                effect_builder,
                deploy,
                source,
                prestate_hash,
                maybe_balance_value,
                maybe_responder,
            ),
            Event::VerifyPaymentLogic {
                deploy,
                source,
                prestate_hash,
                maybe_responder,
            } => self.verify_payment_logic(
                effect_builder,
                deploy,
                source,
                prestate_hash,
                maybe_responder,
            ),
            Event::VerifySessionLogic {
                deploy,
                source,
                prestate_hash,
                maybe_responder,
            } => self.verify_session_logic(
                effect_builder,
                deploy,
                source,
                prestate_hash,
                maybe_responder,
            ),
            Event::GetContractResult {
                deploy,
                source,
                prestate_hash,
                is_payment,
                contract_identifier,
                maybe_contract,
                maybe_responder,
            } => {
                let entry_point = if is_payment {
                    deploy.payment().entry_point_name()
                } else {
                    deploy.session().entry_point_name()
                }
                .to_string();
                self.handle_get_contract_result(
                    effect_builder,
                    deploy,
                    source,
                    prestate_hash,
                    is_payment,
                    entry_point,
                    contract_identifier,
                    maybe_contract,
                    maybe_responder,
                )
            }
            Event::GetContractPackageResult {
                deploy,
                source,
                prestate_hash,
                is_payment,
                contract_package_identifier,
                maybe_contract_package,
                maybe_responder,
            } => self.handle_get_contract_package_result(
                effect_builder,
                deploy,
                source,
                prestate_hash,
                is_payment,
                contract_package_identifier,
                maybe_contract_package,
                maybe_responder,
            ),
            Event::VerifyDeployCryptographicValidity {
                deploy,
                source,
                maybe_responder,
            } => self.validate_deploy_cryptography(effect_builder, deploy, source, maybe_responder),
            Event::PutToStorageResult {
                deploy,
                source,
                is_new,
                maybe_responder,
            } => {
                self.handle_put_to_storage(effect_builder, deploy, source, is_new, maybe_responder)
            }
            Event::InvalidDeployResult {
                deploy,
                source,
                error,
                maybe_responder,
            } => {
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
