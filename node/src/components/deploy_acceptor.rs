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
        chainspec::DeployConfig, BlockHeader, Chainspec, Deploy, DeployConfigurationFailure,
        Timestamp,
    },
    utils::Source,
    NodeRng,
};

pub(crate) use event::{Event, EventMetadata};

const ARG_TARGET: &str = "target";

#[derive(Debug, Error, Serialize)]
pub(crate) enum Error {
    /// The block chain has no blocks.
    #[error("block chain has no blocks")]
    EmptyBlockchain,

    /// The deploy is invalid due to failing to meet the deploy configuration.
    #[error("invalid deploy: {0}")]
    InvalidDeployConfiguration(DeployConfigurationFailure),

    /// The deploy is invalid due to missing or otherwise invalid parameters.
    #[error("deploy parameter failure: {failure} at prestate_hash: {prestate_hash}")]
    InvalidDeployParameters {
        prestate_hash: Digest,
        failure: DeployParameterFailure,
    },

    /// The deploy received by the node from the client has expired.
    #[error("deploy received by the node expired at {deploy_expiry_timestamp} with node's time at {current_node_timestamp}")]
    ExpiredDeploy {
        /// The timestamp when the deploy expires.
        deploy_expiry_timestamp: Timestamp,
        /// The timestamp when the node validated the expiry timestamp.
        current_node_timestamp: Timestamp,
    },
}

/// A representation of the way in which a deploy failed validation checks.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
pub(crate) enum DeployParameterFailure {
    /// Account does not exist.
    #[error("account {account_hash} does not exist")]
    NonexistentAccount { account_hash: AccountHash },
    /// Nonexistent contract at hash.
    #[error("contract at {contract_hash} does not exist")]
    NonexistentContractAtHash { contract_hash: ContractHash },
    /// Nonexistent contract entrypoint.
    #[error("contract does not have {entry_point}")]
    NonexistentContractEntryPoint { entry_point: String },
    /// Contract Package does not exist.
    #[error("contract package at {contract_package_hash} does not exist")]
    NonexistentContractPackageAtHash {
        contract_package_hash: ContractPackageHash,
    },
    /// Invalid contract at given version.
    #[error("invalid contract at version: {contract_version}")]
    InvalidContractAtVersion { contract_version: ContractVersion },
    /// Invalid associated keys.
    #[error("account authorization invalid")]
    InvalidAssociatedKeys,
    /// Insufficient deploy signature weight.
    #[error("insufficient deploy signature weight")]
    InsufficientDeploySignatureWeight,
    /// The deploy's account has insufficient balance.
    #[error("insufficient balance in account {account_hash}")]
    InsufficientBalance { account_hash: AccountHash },
    /// The deploy's account has an unknown balance.
    #[error("unable to determine balance for {account_hash}")]
    UnknownBalance { account_hash: AccountHash },
    /// Transfer is not valid for payment code.
    #[error("transfer is not valid for payment code")]
    InvalidPaymentVariant,
    /// Missing payment "amount" runtime argument.
    #[error("missing payment 'amount' runtime argument")]
    MissingPaymentAmount,
    /// Failed to parse payment "amount" runtime argument.
    #[error("failed to parse payment 'amount' runtime argument as U512")]
    FailedToParsePaymentAmount,
    /// Missing transfer "target" runtime argument.
    #[error("missing transfer 'target' runtime argument")]
    MissingTransferTarget,
    /// Module bytes for session code cannot be empty.
    #[error("module bytes for session code cannot be empty")]
    MissingModuleBytes,
}

/// A helper trait constraining `DeployAcceptor` compatible reactor events.
pub(crate) trait ReactorEventT:
    From<Event>
    + From<DeployAcceptorAnnouncement>
    + From<StorageRequest>
    + From<ContractRuntimeRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<DeployAcceptorAnnouncement>
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
    max_associated_keys: u32,
    metrics: metrics::Metrics,
}

impl DeployAcceptor {
    pub(crate) fn new(
        chainspec: &Chainspec,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(DeployAcceptor {
            chain_name: chainspec.network_config.name.clone(),
            protocol_version: chainspec.protocol_version(),
            deploy_config: chainspec.deploy_config,
            max_associated_keys: chainspec.core_config.max_associated_keys,
            metrics: metrics::Metrics::new(registry)?,
        })
    }

    /// Handles receiving a new `Deploy` from a peer or client.
    /// In the case of a peer, there should be no responder and the variant should be `None`
    /// In the case of a client, there should be a responder to communicate the validity of the
    /// deploy and the variant will be `Some`
    fn accept<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        deploy: Box<Deploy>,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        let verification_start_timestamp = Timestamp::now();
        let acceptable_result = deploy.is_config_compliant(
            &self.chain_name,
            &self.deploy_config,
            self.max_associated_keys,
        );
        // checks chainspec values
        // DOES NOT check cryptographic security
        if let Err(error) = acceptable_result {
            debug!(%deploy, %error, "deploy is incorrectly configured");
            return self.handle_invalid_deploy_result(
                effect_builder,
                EventMetadata::new(deploy, source, maybe_responder),
                Error::InvalidDeployConfiguration(error),
                verification_start_timestamp,
            );
        }

        // We only perform expiry checks on deploys received from the client.
        if source.is_client() {
            let current_node_timestamp = Timestamp::now();
            if deploy.header().expired(current_node_timestamp) {
                let time_of_expiry = deploy.header().expires();
                debug!(%deploy, "deploy has expired");
                return self.handle_invalid_deploy_result(
                    effect_builder,
                    EventMetadata::new(deploy, source, maybe_responder),
                    Error::ExpiredDeploy {
                        deploy_expiry_timestamp: time_of_expiry,
                        current_node_timestamp,
                    },
                    verification_start_timestamp,
                );
            }
        }

        effect_builder
            .get_highest_block_header_from_storage()
            .event(move |maybe_block_header| Event::GetBlockHeaderResult {
                event_metadata: EventMetadata::new(deploy, source, maybe_responder),
                maybe_block_header: Box::new(maybe_block_header),
                verification_start_timestamp,
            })
    }

    fn handle_get_block_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        maybe_block: Option<BlockHeader>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let mut effects = Effects::new();

        let block = match maybe_block {
            Some(block) => block,
            None => {
                // this should be unreachable per current design of the system
                if let Some(responder) = event_metadata.maybe_responder {
                    effects.extend(responder.respond(Err(Error::EmptyBlockchain)).ignore());
                }
                return effects;
            }
        };

        let prestate_hash = *block.state_root_hash();
        let account_hash = event_metadata.deploy.header().account().to_account_hash();
        let account_key = account_hash.into();

        if event_metadata.source.is_client() {
            effect_builder
                .get_account_from_global_state(prestate_hash, account_key)
                .event(move |maybe_account| Event::GetAccountResult {
                    event_metadata,
                    maybe_account,
                    prestate_hash,
                    verification_start_timestamp,
                })
        } else {
            self.verify_payment_logic(
                effect_builder,
                event_metadata,
                prestate_hash,
                verification_start_timestamp,
            )
        }
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
                let account_hash = event_metadata.deploy.header().account().to_account_hash();
                let error = Error::InvalidDeployParameters {
                    prestate_hash,
                    failure: DeployParameterFailure::NonexistentAccount { account_hash },
                };
                debug!(
                    ?account_hash,
                    "nonexistent account associated with the deploy"
                );
                self.handle_invalid_deploy_result(
                    effect_builder,
                    event_metadata,
                    error,
                    verification_start_timestamp,
                )
            }
            Some(account) => {
                let authorization_keys = event_metadata
                    .deploy
                    .approvals()
                    .iter()
                    .map(|approval| approval.signer().to_account_hash())
                    .collect();
                if !account.can_authorize(&authorization_keys) {
                    let error = Error::InvalidDeployParameters {
                        prestate_hash,
                        failure: DeployParameterFailure::InvalidAssociatedKeys,
                    };
                    debug!(?authorization_keys, "account authorization invalid");
                    return self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        error,
                        verification_start_timestamp,
                    );
                }
                if !account.can_deploy_with(&authorization_keys) {
                    let error = Error::InvalidDeployParameters {
                        prestate_hash,
                        failure: DeployParameterFailure::InsufficientDeploySignatureWeight,
                    };
                    debug!(?authorization_keys, "insufficient deploy signature weight");
                    return self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        error,
                        verification_start_timestamp,
                    );
                }
                effect_builder
                    .check_purse_balance(prestate_hash, account.main_purse())
                    .event(move |maybe_balance_value| Event::GetBalanceResult {
                        event_metadata,
                        prestate_hash,
                        maybe_balance_value,
                        account_hash: account.account_hash(),
                        verification_start_timestamp,
                    })
            }
        }
    }

    fn handle_get_balance_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        maybe_balance_value: Option<U512>,
        account_hash: AccountHash,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        if !event_metadata.source.is_client() {
            // This would only happen due to programmer error and should crash the node.
            // Balance checks for deploys received by from a peer will cause the network
            // to stall.
            // TODO: Change this to a fatal!
            panic!("Balance checks for deploys received from peers should never occur.")
        }
        match maybe_balance_value {
            None => {
                let error = Error::InvalidDeployParameters {
                    prestate_hash,
                    failure: DeployParameterFailure::UnknownBalance { account_hash },
                };
                debug!(?account_hash, "unable to determine balance");
                self.handle_invalid_deploy_result(
                    effect_builder,
                    event_metadata,
                    error,
                    verification_start_timestamp,
                )
            }
            Some(balance) => {
                let has_minimum_balance = balance >= *MAX_PAYMENT;
                if !has_minimum_balance {
                    let error = Error::InvalidDeployParameters {
                        prestate_hash,
                        failure: DeployParameterFailure::InsufficientBalance { account_hash },
                    };
                    debug!(?account_hash, "insufficient balance");
                    self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        error,
                        verification_start_timestamp,
                    )
                } else {
                    self.verify_payment_logic(
                        effect_builder,
                        event_metadata,
                        prestate_hash,
                        verification_start_timestamp,
                    )
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
        let payment = event_metadata.deploy.payment();

        let make_error = |failure: DeployParameterFailure| Error::InvalidDeployParameters {
            prestate_hash,
            failure,
        };

        match payment {
            ExecutableDeployItem::Transfer { .. } => {
                debug!("invalid payment variant in payment logic");
                return self.handle_invalid_deploy_result(
                    effect_builder,
                    event_metadata,
                    make_error(DeployParameterFailure::InvalidPaymentVariant),
                    verification_start_timestamp,
                );
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                // module bytes being empty implies the payment executable is standard payment.
                if module_bytes.is_empty() {
                    if let Some(value) = args.get(ARG_AMOUNT) {
                        if value.clone().into_t::<U512>().is_err() {
                            debug!("failed to parse payment amount in payment logic");
                            return self.handle_invalid_deploy_result(
                                effect_builder,
                                event_metadata,
                                make_error(DeployParameterFailure::FailedToParsePaymentAmount),
                                verification_start_timestamp,
                            );
                        }
                    } else {
                        debug!("payment amount missing in payment logic");
                        return self.handle_invalid_deploy_result(
                            effect_builder,
                            event_metadata,
                            make_error(DeployParameterFailure::MissingPaymentAmount),
                            verification_start_timestamp,
                        );
                    }
                }
            }
            ExecutableDeployItem::StoredContractByHash { .. }
            | ExecutableDeployItem::StoredContractByName { .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { .. }
            | ExecutableDeployItem::StoredVersionedContractByName { .. } => (),
        }

        match payment.identifier() {
            // We skip validation if the identifier is a named key, since that could yield a
            // validation success at block X, then a validation failure at block X+1 (e.g. if the
            // named key is deleted, or updated to point to an item which will fail subsequent
            // validation).
            ExecutableDeployItemIdentifier::Module
            | ExecutableDeployItemIdentifier::Transfer
            | ExecutableDeployItemIdentifier::Contract(ContractIdentifier::Name(_))
            | ExecutableDeployItemIdentifier::Package(ContractPackageIdentifier::Name { .. }) => {
                self.verify_session_logic(
                    effect_builder,
                    event_metadata,
                    prestate_hash,
                    verification_start_timestamp,
                )
            }
            ExecutableDeployItemIdentifier::Contract(ContractIdentifier::Hash(contract_hash)) => {
                let query_key = Key::from(contract_hash);
                let path = vec![];
                effect_builder
                    .get_contract_for_validation(prestate_hash, query_key, path)
                    .event(move |maybe_contract| Event::GetContractResult {
                        event_metadata,
                        prestate_hash,
                        is_payment: true,
                        contract_hash,
                        maybe_contract,
                        verification_start_timestamp,
                    })
            }
            ExecutableDeployItemIdentifier::Package(
                ref contract_package_identifier @ ContractPackageIdentifier::Hash {
                    contract_package_hash,
                    ..
                },
            ) => {
                let query_key = Key::from(contract_package_hash);
                let path = vec![];
                let maybe_package_version = contract_package_identifier.version();
                effect_builder
                    .get_contract_package_for_validation(prestate_hash, query_key, path)
                    .event(
                        move |maybe_contract_package| Event::GetContractPackageResult {
                            event_metadata,
                            prestate_hash,
                            is_payment: true,
                            contract_package_hash,
                            maybe_package_version,
                            maybe_contract_package,
                            verification_start_timestamp,
                        },
                    )
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
        let session = event_metadata.deploy.session();

        let make_error = |failure: DeployParameterFailure| Error::InvalidDeployParameters {
            prestate_hash,
            failure,
        };

        match session {
            ExecutableDeployItem::Transfer { args } => {
                // We rely on the `Deploy::is_config_compliant` to check
                // that the transfer amount arg is present and is a valid U512.

                if args.get(ARG_TARGET).is_none() {
                    debug!("native transfer object is missing transfer argument");
                    return self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        make_error(DeployParameterFailure::MissingTransferTarget),
                        verification_start_timestamp,
                    );
                }
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. } => {
                if module_bytes.is_empty() {
                    debug!("module bytes in session logic is empty");
                    return self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        make_error(DeployParameterFailure::MissingModuleBytes),
                        verification_start_timestamp,
                    );
                }
            }
            ExecutableDeployItem::StoredContractByHash { .. }
            | ExecutableDeployItem::StoredContractByName { .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { .. }
            | ExecutableDeployItem::StoredVersionedContractByName { .. } => (),
        }

        match session.identifier() {
            // We skip validation if the identifier is a named key, since that could yield a
            // validation success at block X, then a validation failure at block X+1 (e.g. if the
            // named key is deleted, or updated to point to an item which will fail subsequent
            // validation).
            ExecutableDeployItemIdentifier::Module
            | ExecutableDeployItemIdentifier::Transfer
            | ExecutableDeployItemIdentifier::Contract(ContractIdentifier::Name(_))
            | ExecutableDeployItemIdentifier::Package(ContractPackageIdentifier::Name { .. }) => {
                self.validate_deploy_cryptography(
                    effect_builder,
                    event_metadata,
                    verification_start_timestamp,
                )
            }
            ExecutableDeployItemIdentifier::Contract(ContractIdentifier::Hash(contract_hash)) => {
                let query_key = Key::from(contract_hash);
                let path = vec![];
                effect_builder
                    .get_contract_for_validation(prestate_hash, query_key, path)
                    .event(move |maybe_contract| Event::GetContractResult {
                        event_metadata,
                        prestate_hash,
                        is_payment: false,
                        contract_hash,
                        maybe_contract,
                        verification_start_timestamp,
                    })
            }
            ExecutableDeployItemIdentifier::Package(
                ref contract_package_identifier @ ContractPackageIdentifier::Hash {
                    contract_package_hash,
                    ..
                },
            ) => {
                let query_key = Key::from(contract_package_hash);
                let path = vec![];
                let maybe_package_version = contract_package_identifier.version();
                effect_builder
                    .get_contract_package_for_validation(prestate_hash, query_key, path)
                    .event(
                        move |maybe_contract_package| Event::GetContractPackageResult {
                            event_metadata,
                            prestate_hash,
                            is_payment: false,
                            contract_package_hash,
                            maybe_package_version,
                            maybe_contract_package,
                            verification_start_timestamp,
                        },
                    )
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
        contract_hash: ContractHash,
        maybe_contract: Option<Contract>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        if let Some(contract) = maybe_contract {
            if !contract.entry_points().has_entry_point(&entry_point) {
                debug!(
                    ?entry_point,
                    ?contract_hash,
                    ?prestate_hash,
                    "missing entry point in contract"
                );
                let error = Error::InvalidDeployParameters {
                    prestate_hash,
                    failure: DeployParameterFailure::NonexistentContractEntryPoint { entry_point },
                };
                return self.handle_invalid_deploy_result(
                    effect_builder,
                    event_metadata,
                    error,
                    verification_start_timestamp,
                );
            }
            if is_payment {
                return self.verify_session_logic(
                    effect_builder,
                    event_metadata,
                    prestate_hash,
                    verification_start_timestamp,
                );
            }
            return self.validate_deploy_cryptography(
                effect_builder,
                event_metadata,
                verification_start_timestamp,
            );
        }

        debug!(?contract_hash, "nonexistent contract with hash");
        let failure = DeployParameterFailure::NonexistentContractAtHash { contract_hash };
        let error = Error::InvalidDeployParameters {
            prestate_hash,
            failure,
        };
        self.handle_invalid_deploy_result(
            effect_builder,
            event_metadata,
            error,
            verification_start_timestamp,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_get_contract_package_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        prestate_hash: Digest,
        is_payment: bool,
        contract_package_hash: ContractPackageHash,
        maybe_package_version: Option<ContractVersion>,
        maybe_contract_package: Option<ContractPackage>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        match maybe_contract_package {
            None => {
                debug!(
                    ?contract_package_hash,
                    "nonexistent contract package with hash"
                );
                let failure = DeployParameterFailure::NonexistentContractPackageAtHash {
                    contract_package_hash,
                };
                let error = Error::InvalidDeployParameters {
                    prestate_hash,
                    failure,
                };
                self.handle_invalid_deploy_result(
                    effect_builder,
                    event_metadata,
                    error,
                    verification_start_timestamp,
                )
            }
            Some(contract_package) => match maybe_package_version {
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
                                    contract_hash,
                                    maybe_contract,
                                    verification_start_timestamp,
                                })
                        }
                        None => {
                            debug!(?contract_version, "invalid contract at version");
                            let error = Error::InvalidDeployParameters {
                                prestate_hash,
                                failure: DeployParameterFailure::InvalidContractAtVersion {
                                    contract_version,
                                },
                            };
                            self.handle_invalid_deploy_result(
                                effect_builder,
                                event_metadata,
                                error,
                                verification_start_timestamp,
                            )
                        }
                    }
                }
                // We continue to the next step in None case due to the subjective
                // nature of global state.
                None => {
                    if is_payment {
                        // This function can be used to validate both session and payment
                        // code. However, there is a call order where payment code must
                        // be verified first before verifying session code.
                        self.verify_session_logic(
                            effect_builder,
                            event_metadata,
                            prestate_hash,
                            verification_start_timestamp,
                        )
                    } else {
                        self.validate_deploy_cryptography(
                            effect_builder,
                            event_metadata,
                            verification_start_timestamp,
                        )
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
        if let Err(deploy_configuration_failure) = event_metadata.deploy.is_valid() {
            // The client has submitted a deploy with one or more invalid signatures.
            // Return an error to the RPC component via the responder.
            debug!("deploy is cryptographically invalid");
            return self.handle_invalid_deploy_result(
                effect_builder,
                event_metadata,
                Error::InvalidDeployConfiguration(deploy_configuration_failure),
                verification_start_timestamp,
            );
        }

        effect_builder
            .put_deploy_to_storage(Box::new((*event_metadata.deploy).clone()))
            .event(move |is_new| Event::PutToStorageResult {
                event_metadata,
                is_new,
                verification_start_timestamp,
            })
    }

    fn handle_invalid_deploy_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        error: Error,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let EventMetadata {
            deploy,
            source,
            maybe_responder,
        } = event_metadata;
        self.metrics.observe_rejected(verification_start_timestamp);
        let mut effects = Effects::new();
        if let Some(responder) = maybe_responder {
            // The client has submitted an invalid deploy
            // Return an error to the RPC component via the responder.
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
            Event::GetBlockHeaderResult {
                event_metadata,
                maybe_block_header,
                verification_start_timestamp,
            } => self.handle_get_block_result(
                effect_builder,
                event_metadata,
                *maybe_block_header,
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
                account_hash,
                verification_start_timestamp,
            } => self.handle_get_balance_result(
                effect_builder,
                event_metadata,
                prestate_hash,
                maybe_balance_value,
                account_hash,
                verification_start_timestamp,
            ),
            Event::GetContractResult {
                event_metadata,
                prestate_hash,
                is_payment,
                contract_hash,
                maybe_contract,
                verification_start_timestamp,
            } => {
                let entry_point = if is_payment {
                    event_metadata.deploy.payment().entry_point_name()
                } else {
                    event_metadata.deploy.session().entry_point_name()
                }
                .to_string();
                self.handle_get_contract_result(
                    effect_builder,
                    event_metadata,
                    prestate_hash,
                    is_payment,
                    entry_point,
                    contract_hash,
                    maybe_contract,
                    verification_start_timestamp,
                )
            }
            Event::GetContractPackageResult {
                event_metadata,
                prestate_hash,
                is_payment,
                contract_package_hash,
                maybe_package_version,
                maybe_contract_package,
                verification_start_timestamp,
            } => self.handle_get_contract_package_result(
                effect_builder,
                event_metadata,
                prestate_hash,
                is_payment,
                contract_package_hash,
                maybe_package_version,
                maybe_contract_package,
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
        }
    }
}
