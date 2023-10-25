#![allow(clippy::boxed_local)] // We use boxed locals to pass on event data unchanged.

mod config;
mod event;
mod metrics;
mod tests;

use std::{collections::BTreeSet, fmt::Debug, sync::Arc};

use datasize::DataSize;
use prometheus::Registry;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, trace};

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
    ContractVersionKey, Key, ProtocolVersion, Timestamp, U512,
};

use crate::{
    components::Component,
    effect::{
        announcements::{DeployAcceptorAnnouncement, FatalAnnouncement},
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal,
    types::{
        chainspec::{CoreConfig, DeployConfig},
        BlockHash, BlockHeader, Chainspec, Deploy, DeployConfigurationFailure, FinalizedApprovals,
    },
    utils::Source,
    NodeRng,
};

pub(crate) use config::Config;
pub(crate) use event::{Event, EventMetadata};

const COMPONENT_NAME: &str = "deploy_acceptor";

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
    #[error(
        "{failure} at state root hash {:?} of block {:?} at height {block_height}",
        state_root_hash,
        block_hash.inner(),
    )]
    InvalidDeployParameters {
        state_root_hash: Digest,
        block_hash: BlockHash,
        block_height: u64,
        failure: DeployParameterFailure,
    },

    /// The deploy received by the node from the client has expired.
    #[error(
        "deploy received by the node expired at {deploy_expiry_timestamp} with node's time at \
        {current_node_timestamp}"
    )]
    ExpiredDeploy {
        /// The timestamp when the deploy expires.
        deploy_expiry_timestamp: Timestamp,
        /// The timestamp when the node validated the expiry timestamp.
        current_node_timestamp: Timestamp,
    },
}

impl Error {
    fn parameter_failure(block_header: &BlockHeader, failure: DeployParameterFailure) -> Self {
        Error::InvalidDeployParameters {
            state_root_hash: *block_header.state_root_hash(),
            block_hash: block_header.block_hash(),
            block_height: block_header.height(),
            failure,
        }
    }
}

/// A representation of the way in which a deploy failed validation checks.
#[derive(Clone, DataSize, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Error, Serialize)]
pub(crate) enum DeployParameterFailure {
    /// Account does not exist.
    #[error("account with hash {account_hash} does not exist")]
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
    #[error("insufficient balance in account with hash {account_hash}")]
    InsufficientBalance { account_hash: AccountHash },
    /// The deploy's account has an unknown balance.
    #[error("unable to determine balance for account with hash {account_hash}")]
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
    + From<FatalAnnouncement>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<DeployAcceptorAnnouncement>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<FatalAnnouncement>
        + Send
{
}

/// The `DeployAcceptor` is the component which handles all new `Deploy`s immediately after they're
/// received by this node, regardless of whether they were provided by a peer or a client.
///
/// It validates a new `Deploy` as far as possible, stores it if valid, then announces the newly-
/// accepted `Deploy`.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
///     style End fill:#66ccff,stroke:#333,stroke-width:4px
///     style Z fill:#ff8a8a,stroke:#333,stroke-width:4px
///     style ZZ fill:#8aff94,stroke:#333,stroke-width:4px
///     title[Deploy Acceptance process]
///     title---Start
///     style title fill:#FFF,stroke:#FFF
///     linkStyle 0 stroke-width:0;
///
///     Start --> A{has valid size?}
///     A -->|Yes| B{"is compliant with config?<br/>(size, chain name, ttl, etc.)"}
///     G -->|Yes| ZZ[Accept]
///     B -->|Yes| C{is from<br/>client?}
///     C -->|Yes| CLIENT{has expired?}
///     B -->|No| Z[Reject]
///     C -->|No| E
///     CLIENT -->|Yes| Z
///     CLIENT -->|No| D{"is client<br/>account correct?<br/>(authorization, weight,<br/>balance, etc.)"}
///     D -->|Yes| E{"is payment and<br/>session logic correct?<br/>(module bytes,<br/>payment amount)"}
///     E -->|No| Z
///     E -->|Yes| F{is contract valid?}
///     F -->|Yes| G{is deploy<br/>cryptographically<br/>valid?}
///     F -->|No| Z
///     D -->|No| Z
///     G -->|No| Z
///     ZZ --> End
///     Z --> End
/// ```
#[derive(Debug, DataSize)]
pub struct DeployAcceptor {
    acceptor_config: Config,
    chain_name: String,
    protocol_version: ProtocolVersion,
    deploy_config: DeployConfig,
    core_config: CoreConfig,
    max_associated_keys: u32,
    #[data_size(skip)]
    metrics: metrics::Metrics,
}

impl DeployAcceptor {
    pub(crate) fn new(
        acceptor_config: Config,
        chainspec: &Chainspec,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(DeployAcceptor {
            acceptor_config,
            chain_name: chainspec.network_config.name.clone(),
            protocol_version: chainspec.protocol_version(),
            deploy_config: chainspec.deploy_config,
            core_config: chainspec.core_config.clone(),
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
        deploy: Arc<Deploy>,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        debug!(%source, %deploy, "checking acceptance");
        let verification_start_timestamp = Timestamp::now();
        let acceptable_result = deploy.is_config_compliant(
            &self.chain_name,
            &self.deploy_config,
            self.max_associated_keys,
            self.acceptor_config.timestamp_leeway,
            verification_start_timestamp,
        );
        // checks chainspec values
        if let Err(error) = acceptable_result {
            debug!(%deploy, %error, "deploy is incorrectly configured");
            return self.handle_invalid_deploy_result(
                effect_builder,
                Box::new(EventMetadata::new(deploy, source, maybe_responder)),
                Error::InvalidDeployConfiguration(error),
                verification_start_timestamp + self.acceptor_config.timestamp_leeway,
            );
        }

        // We only perform expiry checks on deploys received from the client.
        if source.is_client() && deploy.header().expired(verification_start_timestamp) {
            let time_of_expiry = deploy.header().expires();
            debug!(%deploy, "deploy has expired");
            return self.handle_invalid_deploy_result(
                effect_builder,
                Box::new(EventMetadata::new(deploy, source, maybe_responder)),
                Error::ExpiredDeploy {
                    deploy_expiry_timestamp: time_of_expiry,
                    current_node_timestamp: verification_start_timestamp,
                },
                verification_start_timestamp,
            );
        }

        // If this has been received from the speculative exec server, use the block specified in
        // the request, otherwise use the highest complete block.
        if let Source::SpeculativeExec(block_header) = &source {
            let account_hash = deploy.header().account().to_account_hash();
            let account_key = Key::from(account_hash);
            let block_header = block_header.clone();
            return effect_builder
                .get_account_from_global_state(*block_header.state_root_hash(), account_key)
                .event(move |maybe_account| Event::GetAccountResult {
                    event_metadata: Box::new(EventMetadata::new(
                        deploy,
                        source.clone(),
                        maybe_responder,
                    )),
                    maybe_account,
                    block_header,
                    verification_start_timestamp,
                });
        }

        effect_builder
            .get_highest_complete_block_header_from_storage()
            .event(move |maybe_block_header| Event::GetBlockHeaderResult {
                event_metadata: Box::new(EventMetadata::new(
                    deploy,
                    source.clone(),
                    maybe_responder,
                )),
                maybe_block_header: maybe_block_header.map(Box::new),
                verification_start_timestamp,
            })
    }

    fn handle_get_block_header_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        maybe_block_header: Option<Box<BlockHeader>>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let mut effects = Effects::new();

        let block_header = match maybe_block_header {
            Some(block_header) => block_header,
            None => {
                // this should be unreachable per current design of the system
                if let Some(responder) = event_metadata.maybe_responder {
                    effects.extend(responder.respond(Err(Error::EmptyBlockchain)).ignore());
                }
                return effects;
            }
        };

        let account_hash = event_metadata.deploy.header().account().to_account_hash();
        let account_key = Key::from(account_hash);

        if event_metadata.source.is_client() {
            effect_builder
                .get_account_from_global_state(*block_header.state_root_hash(), account_key)
                .event(move |maybe_account| Event::GetAccountResult {
                    event_metadata,
                    maybe_account,
                    block_header,
                    verification_start_timestamp,
                })
        } else {
            self.verify_payment_logic(
                effect_builder,
                event_metadata,
                block_header,
                verification_start_timestamp,
            )
        }
    }

    fn handle_get_account_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        maybe_account: Option<Account>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        match maybe_account {
            None => {
                let account_hash = event_metadata.deploy.header().account().to_account_hash();
                let error = Error::parameter_failure(
                    &block_header,
                    DeployParameterFailure::NonexistentAccount { account_hash },
                );
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

                let admin_set: BTreeSet<AccountHash> = {
                    self.core_config
                        .administrators
                        .iter()
                        .map(|public_key| public_key.to_account_hash())
                        .collect()
                };
                if admin_set.intersection(&authorization_keys).next().is_some() {
                    return effect_builder
                        .check_purse_balance(*block_header.state_root_hash(), account.main_purse())
                        .event(move |maybe_balance_value| Event::GetBalanceResult {
                            event_metadata,
                            maybe_balance_value,
                            account_hash: account.account_hash(),
                            verification_start_timestamp,
                            block_header,
                        });
                }

                if !account.can_authorize(&authorization_keys) {
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::InvalidAssociatedKeys,
                    );
                    debug!(?authorization_keys, "account authorization invalid");
                    return self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        error,
                        verification_start_timestamp,
                    );
                }

                if !account.can_deploy_with(&authorization_keys) {
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::InsufficientDeploySignatureWeight,
                    );
                    debug!(?authorization_keys, "insufficient deploy signature weight");
                    return self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        error,
                        verification_start_timestamp,
                    );
                }
                effect_builder
                    .check_purse_balance(*block_header.state_root_hash(), account.main_purse())
                    .event(move |maybe_balance_value| Event::GetBalanceResult {
                        event_metadata,
                        block_header,
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
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        maybe_balance_value: Option<U512>,
        account_hash: AccountHash,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        if !event_metadata.source.is_client() {
            // This would only happen due to programmer error and should crash the node.
            // Balance checks for deploys received by from a peer will cause the network
            // to stall.
            return fatal!(
                effect_builder,
                "Balance checks for deploys received from peers should never occur."
            )
            .ignore();
        }
        match maybe_balance_value {
            None => {
                let error = Error::parameter_failure(
                    &block_header,
                    DeployParameterFailure::UnknownBalance { account_hash },
                );
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
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::InsufficientBalance { account_hash },
                    );
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
                        block_header,
                        verification_start_timestamp,
                    )
                }
            }
        }
    }

    fn verify_payment_logic<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let payment = event_metadata.deploy.payment();
        match payment {
            ExecutableDeployItem::Transfer { .. } => {
                let error = Error::parameter_failure(
                    &block_header,
                    DeployParameterFailure::InvalidPaymentVariant,
                );
                debug!("invalid payment variant in payment logic");
                return self.handle_invalid_deploy_result(
                    effect_builder,
                    event_metadata,
                    error,
                    verification_start_timestamp,
                );
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                // module bytes being empty implies the payment executable is standard payment.
                if module_bytes.is_empty() {
                    if let Some(value) = args.get(ARG_AMOUNT) {
                        if value.clone().into_t::<U512>().is_err() {
                            let error = Error::parameter_failure(
                                &block_header,
                                DeployParameterFailure::FailedToParsePaymentAmount,
                            );
                            debug!("failed to parse payment amount in payment logic");
                            return self.handle_invalid_deploy_result(
                                effect_builder,
                                event_metadata,
                                error,
                                verification_start_timestamp,
                            );
                        }
                    } else {
                        let error = Error::parameter_failure(
                            &block_header,
                            DeployParameterFailure::MissingPaymentAmount,
                        );
                        debug!("payment amount missing in payment logic");
                        return self.handle_invalid_deploy_result(
                            effect_builder,
                            event_metadata,
                            error,
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
                    block_header,
                    verification_start_timestamp,
                )
            }
            ExecutableDeployItemIdentifier::Contract(ContractIdentifier::Hash(contract_hash)) => {
                let query_key = Key::from(contract_hash);
                let path = vec![];
                effect_builder
                    .get_contract_for_validation(*block_header.state_root_hash(), query_key, path)
                    .event(move |maybe_contract| Event::GetContractResult {
                        event_metadata,
                        block_header,
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
                    .get_contract_package_for_validation(
                        *block_header.state_root_hash(),
                        query_key,
                        path,
                    )
                    .event(
                        move |maybe_contract_package| Event::GetContractPackageResult {
                            event_metadata,
                            block_header,
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
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let session = event_metadata.deploy.session();

        match session {
            ExecutableDeployItem::Transfer { args } => {
                // We rely on the `Deploy::is_config_compliant` to check
                // that the transfer amount arg is present and is a valid U512.

                if args.get(ARG_TARGET).is_none() {
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::MissingTransferTarget,
                    );
                    debug!("native transfer object is missing transfer argument");
                    return self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        error,
                        verification_start_timestamp,
                    );
                }
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. } => {
                if module_bytes.is_empty() {
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::MissingModuleBytes,
                    );
                    debug!("module bytes in session logic is empty");
                    return self.handle_invalid_deploy_result(
                        effect_builder,
                        event_metadata,
                        error,
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
                    .get_contract_for_validation(*block_header.state_root_hash(), query_key, path)
                    .event(move |maybe_contract| Event::GetContractResult {
                        event_metadata,
                        block_header,
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
                    .get_contract_package_for_validation(
                        *block_header.state_root_hash(),
                        query_key,
                        path,
                    )
                    .event(
                        move |maybe_contract_package| Event::GetContractPackageResult {
                            event_metadata,
                            block_header,
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
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        entry_point: String,
        contract_hash: ContractHash,
        maybe_contract: Option<Box<Contract>>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        if let Some(contract) = maybe_contract {
            if !contract.entry_points().has_entry_point(&entry_point) {
                debug!(
                    ?entry_point,
                    ?contract_hash,
                    state_root_hash = ?block_header.state_root_hash(),
                    "missing entry point in contract"
                );
                let error = Error::parameter_failure(
                    &block_header,
                    DeployParameterFailure::NonexistentContractEntryPoint { entry_point },
                );
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
                    block_header,
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
        let error = Error::parameter_failure(
            &block_header,
            DeployParameterFailure::NonexistentContractAtHash { contract_hash },
        );
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
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        contract_package_hash: ContractPackageHash,
        maybe_package_version: Option<ContractVersion>,
        maybe_contract_package: Option<Box<ContractPackage>>,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        match maybe_contract_package {
            None => {
                debug!(
                    ?contract_package_hash,
                    "nonexistent contract package with hash"
                );
                let error = Error::parameter_failure(
                    &block_header,
                    DeployParameterFailure::NonexistentContractPackageAtHash {
                        contract_package_hash,
                    },
                );
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
                                .get_contract_for_validation(
                                    *block_header.state_root_hash(),
                                    query_key,
                                    vec![],
                                )
                                .event(move |maybe_contract| Event::GetContractResult {
                                    event_metadata,
                                    block_header,
                                    is_payment,
                                    contract_hash,
                                    maybe_contract,
                                    verification_start_timestamp,
                                })
                        }
                        None => {
                            debug!(?contract_version, "invalid contract at version");
                            let error = Error::parameter_failure(
                                &block_header,
                                DeployParameterFailure::InvalidContractAtVersion {
                                    contract_version,
                                },
                            );
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
                            block_header,
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

    fn validate_deploy_cryptography<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
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

        // If this has been received from the speculative exec server, we just want to call the
        // responder and finish.  Otherwise store the deploy and announce it if required.
        if let Source::SpeculativeExec(_) = event_metadata.source {
            let effects = if let Some(responder) = event_metadata.maybe_responder {
                responder.respond(Ok(())).ignore()
            } else {
                error!("speculative exec source should always have a responder");
                Effects::new()
            };
            return effects;
        }

        effect_builder
            .put_deploy_to_storage(event_metadata.deploy.clone())
            .event(move |is_new| Event::PutToStorageResult {
                event_metadata,
                is_new,
                verification_start_timestamp,
            })
    }

    fn handle_invalid_deploy_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        error: Error,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let EventMetadata {
            deploy,
            source,
            maybe_responder,
        } = *event_metadata;
        if !matches!(source, Source::SpeculativeExec(_)) {
            self.metrics.observe_rejected(verification_start_timestamp);
        }
        let mut effects = Effects::new();
        if let Some(responder) = maybe_responder {
            // The client has submitted an invalid deploy
            // Return an error to the RPC component via the responder.
            effects.extend(responder.respond(Err(error)).ignore());
        }

        // If this has NOT been received from the speculative exec server, announce it.
        if !matches!(source, Source::SpeculativeExec(_)) {
            effects.extend(
                effect_builder
                    .announce_invalid_deploy(deploy, source)
                    .ignore(),
            );
        }
        effects
    }

    fn handle_put_to_storage<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        is_new: bool,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        let mut effects = Effects::new();
        if is_new {
            effects.extend(
                effect_builder
                    .announce_new_deploy_accepted(event_metadata.deploy, event_metadata.source)
                    .ignore(),
            );
        } else if matches!(event_metadata.source, Source::Peer(_)) {
            // If `is_new` is `false`, the deploy was previously stored.  If the source is `Peer`,
            // we got here as a result of a Fetch<Deploy>, and the incoming deploy could have a
            // different set of approvals to the one already stored.  We can treat the incoming
            // approvals as finalized and now try and store them.  If storing them returns `true`,
            // (indicating the approvals are different to any previously stored) we can announce a
            // new deploy accepted, causing the fetcher to be notified.
            return effect_builder
                .store_finalized_approvals(
                    *event_metadata.deploy.hash(),
                    FinalizedApprovals::new(event_metadata.deploy.approvals().clone()),
                )
                .event(move |is_new| Event::StoredFinalizedApprovals {
                    event_metadata,
                    is_new,
                    verification_start_timestamp,
                });
        }
        self.metrics.observe_accepted(verification_start_timestamp);

        // success
        if let Some(responder) = event_metadata.maybe_responder {
            effects.extend(responder.respond(Ok(())).ignore());
        }
        effects
    }

    fn handle_stored_finalized_approvals<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        is_new: bool,
        verification_start_timestamp: Timestamp,
    ) -> Effects<Event> {
        self.metrics.observe_accepted(verification_start_timestamp);
        let EventMetadata {
            deploy,
            source,
            maybe_responder,
        } = *event_metadata;
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
}

impl<REv: ReactorEventT> Component<REv> for DeployAcceptor {
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!(?event, "DeployAcceptor: handling event");
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
            } => self.handle_get_block_header_result(
                effect_builder,
                event_metadata,
                maybe_block_header,
                verification_start_timestamp,
            ),
            Event::GetAccountResult {
                event_metadata,
                block_header,
                maybe_account,
                verification_start_timestamp,
            } => self.handle_get_account_result(
                effect_builder,
                event_metadata,
                block_header,
                maybe_account,
                verification_start_timestamp,
            ),
            Event::GetBalanceResult {
                event_metadata,
                block_header,
                maybe_balance_value,
                account_hash,
                verification_start_timestamp,
            } => self.handle_get_balance_result(
                effect_builder,
                event_metadata,
                block_header,
                maybe_balance_value,
                account_hash,
                verification_start_timestamp,
            ),
            Event::GetContractResult {
                event_metadata,
                block_header,
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
                    block_header,
                    is_payment,
                    entry_point,
                    contract_hash,
                    maybe_contract,
                    verification_start_timestamp,
                )
            }
            Event::GetContractPackageResult {
                event_metadata,
                block_header,
                is_payment,
                contract_package_hash,
                maybe_package_version,
                maybe_contract_package,
                verification_start_timestamp,
            } => self.handle_get_contract_package_result(
                effect_builder,
                event_metadata,
                block_header,
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
            Event::StoredFinalizedApprovals {
                event_metadata,
                is_new,
                verification_start_timestamp,
            } => self.handle_stored_finalized_approvals(
                effect_builder,
                event_metadata,
                is_new,
                verification_start_timestamp,
            ),
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}
