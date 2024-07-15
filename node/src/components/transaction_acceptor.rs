mod config;
mod error;
mod event;
mod metrics;
mod tests;

use std::{collections::BTreeSet, fmt::Debug, sync::Arc};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{debug, error, trace};

use casper_execution_engine::engine_state::MAX_PAYMENT;
use casper_storage::data_access_layer::{balance::BalanceHandling, BalanceRequest, ProofHandling};
use casper_types::{
    account::AccountHash, addressable_entity::AddressableEntity, system::auction::ARG_AMOUNT,
    AddressableEntityHash, AddressableEntityIdentifier, BlockHeader, Chainspec, EntityAddr,
    EntityVersion, EntityVersionKey, EntryPoint, EntryPointAddr, ExecutableDeployItem,
    ExecutableDeployItemIdentifier, InitiatorAddr, Key, Package, PackageAddr, PackageHash,
    PackageIdentifier, Transaction, TransactionEntryPoint, TransactionInvocationTarget,
    TransactionTarget, DEFAULT_ENTRY_POINT_NAME, U512,
};

use crate::{
    components::Component,
    effect::{
        announcements::{FatalAnnouncement, TransactionAcceptorAnnouncement},
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal,
    utils::Source,
    NodeRng,
};

pub(crate) use config::Config;
pub(crate) use error::{DeployParameterFailure, Error, ParameterFailure};
pub(crate) use event::{Event, EventMetadata};

const COMPONENT_NAME: &str = "transaction_acceptor";

const ARG_TARGET: &str = "target";

/// A helper trait constraining `TransactionAcceptor` compatible reactor events.
pub(crate) trait ReactorEventT:
    From<Event>
    + From<TransactionAcceptorAnnouncement>
    + From<StorageRequest>
    + From<ContractRuntimeRequest>
    + From<FatalAnnouncement>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<TransactionAcceptorAnnouncement>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<FatalAnnouncement>
        + Send
{
}

/// The `TransactionAcceptor` is the component which handles all new `Transaction`s immediately
/// after they're received by this node, regardless of whether they were provided by a peer or a
/// client, unless they were actively retrieved by this node via a fetch request (in which case the
/// fetcher performs the necessary validation and stores it).
///
/// It validates a new `Transaction` as far as possible, stores it if valid, then announces the
/// newly-accepted `Transaction`.
#[derive(Debug, DataSize)]
pub struct TransactionAcceptor {
    acceptor_config: Config,
    chainspec: Arc<Chainspec>,
    administrators: BTreeSet<AccountHash>,
    #[data_size(skip)]
    metrics: metrics::Metrics,
    balance_hold_interval: u64,
}

impl TransactionAcceptor {
    pub(crate) fn new(
        acceptor_config: Config,
        chainspec: Arc<Chainspec>,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        let administrators = chainspec
            .core_config
            .administrators
            .iter()
            .map(|public_key| public_key.to_account_hash())
            .collect();
        let balance_hold_interval = chainspec.core_config.gas_hold_interval.millis();
        Ok(TransactionAcceptor {
            acceptor_config,
            chainspec,
            administrators,
            metrics: metrics::Metrics::new(registry)?,
            balance_hold_interval,
        })
    }

    /// Handles receiving a new `Transaction` from the given source.
    fn accept<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        transaction: Transaction,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        debug!(%source, %transaction, "checking transaction before accepting");
        let event_metadata = Box::new(EventMetadata::new(transaction, source, maybe_responder));

        let is_config_compliant = match &event_metadata.transaction {
            Transaction::Deploy(deploy) => deploy
                .is_config_compliant(
                    &self.chainspec,
                    self.acceptor_config.timestamp_leeway,
                    event_metadata.verification_start_timestamp,
                )
                .map_err(|err| Error::InvalidTransaction(err.into())),
            Transaction::V1(txn) => txn
                .is_config_compliant(
                    &self.chainspec,
                    self.acceptor_config.timestamp_leeway,
                    event_metadata.verification_start_timestamp,
                )
                .map_err(|err| Error::InvalidTransaction(err.into())),
        };

        if let Err(error) = is_config_compliant {
            return self.reject_transaction(effect_builder, *event_metadata, error);
        }

        // We only perform expiry checks on transactions received from the client.
        let current_node_timestamp = event_metadata.verification_start_timestamp;
        if event_metadata.source.is_client()
            && event_metadata.transaction.expired(current_node_timestamp)
        {
            let expiry_timestamp = event_metadata.transaction.expires();
            return self.reject_transaction(
                effect_builder,
                *event_metadata,
                Error::Expired {
                    expiry_timestamp,
                    current_node_timestamp,
                },
            );
        }

        effect_builder
            .get_highest_complete_block_header_from_storage()
            .event(move |maybe_block_header| Event::GetBlockHeaderResult {
                event_metadata,
                maybe_block_header: maybe_block_header.map(Box::new),
            })
    }

    fn handle_get_block_header_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        maybe_block_header: Option<Box<BlockHeader>>,
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

        if event_metadata.source.is_client() {
            let account_hash = match event_metadata.transaction.initiator_addr() {
                InitiatorAddr::PublicKey(public_key) => public_key.to_account_hash(),
                InitiatorAddr::AccountHash(account_hash) => account_hash,
            };
            let entity_addr = EntityAddr::Account(account_hash.value());
            effect_builder
                .get_addressable_entity(*block_header.state_root_hash(), entity_addr)
                .event(move |result| Event::GetAddressableEntityResult {
                    event_metadata,
                    maybe_entity: result.into_option(),
                    block_header,
                })
        } else {
            self.verify_payment(effect_builder, event_metadata, block_header)
        }
    }

    fn handle_get_entity_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        maybe_entity: Option<AddressableEntity>,
    ) -> Effects<Event> {
        match maybe_entity {
            None => {
                let initiator_addr = event_metadata.transaction.initiator_addr();
                let error = Error::parameter_failure(
                    &block_header,
                    ParameterFailure::NoSuchAddressableEntity { initiator_addr },
                );
                self.reject_transaction(effect_builder, *event_metadata, error)
            }
            Some(entity) => {
                if let Err(parameter_failure) =
                    is_authorized_entity(&entity, &self.administrators, &event_metadata)
                {
                    let error = Error::parameter_failure(&block_header, parameter_failure);
                    return self.reject_transaction(effect_builder, *event_metadata, error);
                }
                let protocol_version = block_header.protocol_version();
                let balance_handling = BalanceHandling::Available;
                let proof_handling = ProofHandling::NoProofs;
                let balance_request = BalanceRequest::from_purse(
                    *block_header.state_root_hash(),
                    protocol_version,
                    entity.main_purse(),
                    balance_handling,
                    proof_handling,
                );
                effect_builder
                    .get_balance(balance_request)
                    .event(move |balance_result| Event::GetBalanceResult {
                        event_metadata,
                        block_header,
                        maybe_balance: balance_result.available_balance().copied(),
                    })
            }
        }
    }

    fn handle_get_balance_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        maybe_balance: Option<U512>,
    ) -> Effects<Event> {
        if !event_metadata.source.is_client() {
            // This would only happen due to programmer error and should crash the node. Balance
            // checks for transactions received from a peer will cause the network to stall.
            return fatal!(
                effect_builder,
                "Balance checks for transactions received from peers should never occur."
            )
            .ignore();
        }
        match maybe_balance {
            None => {
                let initiator_addr = event_metadata.transaction.initiator_addr();
                let error = Error::parameter_failure(
                    &block_header,
                    ParameterFailure::UnknownBalance { initiator_addr },
                );
                self.reject_transaction(effect_builder, *event_metadata, error)
            }
            Some(balance) => {
                let has_minimum_balance = balance >= *MAX_PAYMENT;
                if !has_minimum_balance {
                    let initiator_addr = event_metadata.transaction.initiator_addr();
                    let error = Error::parameter_failure(
                        &block_header,
                        ParameterFailure::InsufficientBalance { initiator_addr },
                    );
                    self.reject_transaction(effect_builder, *event_metadata, error)
                } else {
                    self.verify_payment(effect_builder, event_metadata, block_header)
                }
            }
        }
    }

    fn verify_payment<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
    ) -> Effects<Event> {
        // Only deploys need their payment code checked.
        let payment_identifier = if let Transaction::Deploy(deploy) = &event_metadata.transaction {
            if let Err(error) = deploy_payment_is_valid(deploy.payment(), &block_header) {
                return self.reject_transaction(effect_builder, *event_metadata, error);
            }
            deploy.payment().identifier()
        } else {
            return self.verify_body(effect_builder, event_metadata, block_header);
        };

        match payment_identifier {
            // We skip validation if the identifier is a named key, since that could yield a
            // validation success at block X, then a validation failure at block X+1 (e.g. if the
            // named key is deleted, or updated to point to an item which will fail subsequent
            // validation).
            ExecutableDeployItemIdentifier::Module
            | ExecutableDeployItemIdentifier::Transfer
            | ExecutableDeployItemIdentifier::AddressableEntity(
                AddressableEntityIdentifier::Name(_),
            )
            | ExecutableDeployItemIdentifier::Package(PackageIdentifier::Name { .. }) => {
                self.verify_body(effect_builder, event_metadata, block_header)
            }
            ExecutableDeployItemIdentifier::AddressableEntity(
                AddressableEntityIdentifier::Hash(contract_hash),
            ) => {
                let entity_addr = EntityAddr::SmartContract(contract_hash.value());
                effect_builder
                    .get_addressable_entity(*block_header.state_root_hash(), entity_addr)
                    .event(move |result| Event::GetContractResult {
                        event_metadata,
                        block_header,
                        is_payment: true,
                        contract_hash,
                        maybe_entity: result.into_option(),
                    })
            }
            ExecutableDeployItemIdentifier::AddressableEntity(
                AddressableEntityIdentifier::Addr(entity_addr),
            ) => effect_builder
                .get_addressable_entity(*block_header.state_root_hash(), entity_addr)
                .event(move |result| Event::GetAddressableEntityResult {
                    event_metadata,
                    block_header,
                    maybe_entity: result.into_option(),
                }),
            ExecutableDeployItemIdentifier::Package(
                ref contract_package_identifier @ PackageIdentifier::Hash { package_hash, .. },
            ) => {
                let key = Key::from(package_hash);
                let maybe_package_version = contract_package_identifier.version();
                effect_builder
                    .get_package(*block_header.state_root_hash(), key)
                    .event(move |maybe_package| Event::GetPackageResult {
                        event_metadata,
                        block_header,
                        is_payment: true,
                        package_hash,
                        maybe_package_version,
                        maybe_package,
                    })
            }
        }
    }

    fn verify_body<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
    ) -> Effects<Event> {
        match &event_metadata.transaction {
            Transaction::Deploy(_) => {
                self.verify_deploy_session(effect_builder, event_metadata, block_header)
            }
            Transaction::V1(_) => {
                self.verify_transaction_v1_body(effect_builder, event_metadata, block_header)
            }
        }
    }

    fn verify_deploy_session<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
    ) -> Effects<Event> {
        let session = match &event_metadata.transaction {
            Transaction::Deploy(deploy) => deploy.session(),
            Transaction::V1(txn) => {
                error!(%txn, "should only handle deploys in verify_deploy_session");
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::ExpectedDeploy,
                );
            }
        };

        match session {
            ExecutableDeployItem::Transfer { args } => {
                // We rely on the `Deploy::is_config_compliant` to check
                // that the transfer amount arg is present and is a valid U512.
                if args.get(ARG_TARGET).is_none() {
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::MissingTransferTarget.into(),
                    );
                    return self.reject_transaction(effect_builder, *event_metadata, error);
                }
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. } => {
                if module_bytes.is_empty() {
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::MissingModuleBytes.into(),
                    );
                    return self.reject_transaction(effect_builder, *event_metadata, error);
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
            | ExecutableDeployItemIdentifier::AddressableEntity(
                AddressableEntityIdentifier::Name(_),
            )
            | ExecutableDeployItemIdentifier::Package(PackageIdentifier::Name { .. }) => {
                self.validate_transaction_cryptography(effect_builder, event_metadata)
            }
            ExecutableDeployItemIdentifier::AddressableEntity(
                AddressableEntityIdentifier::Hash(entity_hash),
            ) => {
                let entity_addr = EntityAddr::SmartContract(entity_hash.value());
                effect_builder
                    .get_addressable_entity(*block_header.state_root_hash(), entity_addr)
                    .event(move |result| Event::GetContractResult {
                        event_metadata,
                        block_header,
                        is_payment: false,
                        contract_hash: entity_hash,
                        maybe_entity: result.into_option(),
                    })
            }
            ExecutableDeployItemIdentifier::AddressableEntity(
                AddressableEntityIdentifier::Addr(entity_addr),
            ) => effect_builder
                .get_addressable_entity(*block_header.state_root_hash(), entity_addr)
                .event(move |result| Event::GetAddressableEntityResult {
                    event_metadata,
                    block_header,
                    maybe_entity: result.into_option(),
                }),
            ExecutableDeployItemIdentifier::Package(
                ref package_identifier @ PackageIdentifier::Hash { package_hash, .. },
            ) => {
                let key = Key::from(package_hash);
                let maybe_package_version = package_identifier.version();
                effect_builder
                    .get_package(*block_header.state_root_hash(), key)
                    .event(move |maybe_package| Event::GetPackageResult {
                        event_metadata,
                        block_header,
                        is_payment: false,
                        package_hash,
                        maybe_package_version,
                        maybe_package,
                    })
            }
        }
    }

    fn verify_transaction_v1_body<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
    ) -> Effects<Event> {
        enum NextStep {
            GetContract(EntityAddr),
            GetPackage(PackageAddr, Option<EntityVersion>),
            CryptoValidation,
        }

        let next_step = match &event_metadata.transaction {
            Transaction::Deploy(deploy) => {
                error!(
                    %deploy,
                    "should only handle version 1 transactions in verify_transaction_v1_body"
                );
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::ExpectedTransactionV1,
                );
            }
            Transaction::V1(txn) => match txn.target() {
                TransactionTarget::Stored { id, .. } => match id {
                    TransactionInvocationTarget::ByHash(entity_addr) => {
                        NextStep::GetContract(EntityAddr::SmartContract(*entity_addr))
                    }
                    TransactionInvocationTarget::ByPackageHash { addr, version } => {
                        NextStep::GetPackage(*addr, *version)
                    }
                    TransactionInvocationTarget::ByName(_)
                    | TransactionInvocationTarget::ByPackageName { .. } => {
                        NextStep::CryptoValidation
                    }
                },
                TransactionTarget::Native | TransactionTarget::Session { .. } => {
                    NextStep::CryptoValidation
                }
            },
        };

        match next_step {
            NextStep::GetContract(entity_addr) => {
                // Use `Key::Hash` variant so that we try to retrieve the entity as either an
                // AddressableEntity, or fall back to retrieving an un-migrated Contract.
                effect_builder
                    .get_addressable_entity(*block_header.state_root_hash(), entity_addr)
                    .event(move |result| Event::GetContractResult {
                        event_metadata,
                        block_header,
                        is_payment: false,
                        contract_hash: AddressableEntityHash::new(entity_addr.value()),
                        maybe_entity: result.into_option(),
                    })
            }
            NextStep::GetPackage(package_addr, maybe_package_version) => {
                let key = Key::Package(package_addr);
                effect_builder
                    .get_package(*block_header.state_root_hash(), key)
                    .event(move |maybe_package| Event::GetPackageResult {
                        event_metadata,
                        block_header,
                        is_payment: false,
                        package_hash: PackageHash::new(package_addr),
                        maybe_package_version,
                        maybe_package,
                    })
            }
            NextStep::CryptoValidation => {
                self.validate_transaction_cryptography(effect_builder, event_metadata)
            }
        }
    }

    fn handle_get_contract_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        contract_hash: AddressableEntityHash,
        maybe_contract: Option<AddressableEntity>,
    ) -> Effects<Event> {
        if maybe_contract.is_none() {
            let error = Error::parameter_failure(
                &block_header,
                ParameterFailure::NoSuchContractAtHash { contract_hash },
            );
            return self.reject_transaction(effect_builder, *event_metadata, error);
        }

        let maybe_entry_point_name = match &event_metadata.transaction {
            Transaction::Deploy(deploy) if is_payment => {
                Some(deploy.payment().entry_point_name().to_string())
            }
            Transaction::Deploy(deploy) => Some(deploy.session().entry_point_name().to_string()),
            Transaction::V1(_) if is_payment => {
                error!("should not fetch a contract to validate payment logic for transaction v1s");
                None
            }
            Transaction::V1(txn) => match txn.entry_point() {
                TransactionEntryPoint::Call => Some(DEFAULT_ENTRY_POINT_NAME.to_owned()),
                TransactionEntryPoint::Custom(name) => Some(name.clone()),
                TransactionEntryPoint::Transfer
                | TransactionEntryPoint::AddBid
                | TransactionEntryPoint::WithdrawBid
                | TransactionEntryPoint::Delegate
                | TransactionEntryPoint::Undelegate
                | TransactionEntryPoint::Redelegate
                | TransactionEntryPoint::ActivateBid
                | TransactionEntryPoint::ChangeBidPublicKey => None,
            },
        };

        match maybe_entry_point_name {
            Some(entry_point_name) => {
                match EntryPointAddr::new_v1_entry_point_addr(
                    EntityAddr::SmartContract(contract_hash.value()),
                    &entry_point_name,
                ) {
                    Ok(entry_point_addr) => effect_builder
                        .get_entry_point_value(
                            *block_header.state_root_hash(),
                            Key::EntryPoint(entry_point_addr),
                        )
                        .event(move |entry_point_result| Event::GetEntryPointResult {
                            event_metadata,
                            block_header,
                            is_payment,
                            entry_point_name,
                            maybe_entry_point: entry_point_result.into_v1_entry_point(),
                        }),
                    Err(_) => {
                        let error = Error::parameter_failure(
                            &block_header,
                            ParameterFailure::NoSuchEntryPoint { entry_point_name },
                        );
                        self.reject_transaction(effect_builder, *event_metadata, error)
                    }
                }
            }
            None => {
                if is_payment {
                    return self.verify_body(effect_builder, event_metadata, block_header);
                }
                self.validate_transaction_cryptography(effect_builder, event_metadata)
            }
        }
    }

    fn handle_get_entry_point_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        entry_point_name: String,
        maybe_entry_point: Option<EntryPoint>,
    ) -> Effects<Event> {
        let entry_point = match maybe_entry_point {
            Some(entry_point) => entry_point,
            None => {
                let error = Error::parameter_failure(
                    &block_header,
                    ParameterFailure::NoSuchEntryPoint { entry_point_name },
                );
                return self.reject_transaction(effect_builder, *event_metadata, error);
            }
        };

        if entry_point_name != *entry_point.name() {
            let error = Error::parameter_failure(
                &block_header,
                ParameterFailure::NoSuchEntryPoint { entry_point_name },
            );
            return self.reject_transaction(effect_builder, *event_metadata, error);
        };

        if is_payment {
            return self.verify_body(effect_builder, event_metadata, block_header);
        }
        self.validate_transaction_cryptography(effect_builder, event_metadata)
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_get_package_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        package_hash: PackageHash,
        maybe_contract_version: Option<EntityVersion>,
        maybe_package: Option<Box<Package>>,
    ) -> Effects<Event> {
        let package = match maybe_package {
            Some(package) => package,
            None => {
                let error = Error::parameter_failure(
                    &block_header,
                    ParameterFailure::NoSuchPackageAtHash { package_hash },
                );
                return self.reject_transaction(effect_builder, *event_metadata, error);
            }
        };

        let entity_version = match maybe_contract_version {
            Some(version) => version,
            None => {
                // We continue to the next step in None case due to the subjective
                // nature of global state.
                if is_payment {
                    return self.verify_body(effect_builder, event_metadata, block_header);
                }
                return self.validate_transaction_cryptography(effect_builder, event_metadata);
            }
        };

        let entity_version_key = EntityVersionKey::new(
            self.chainspec.protocol_config.version.value().major,
            entity_version,
        );

        if package.is_version_missing(entity_version_key) {
            let error = Error::parameter_failure(
                &block_header,
                ParameterFailure::MissingEntityAtVersion { entity_version },
            );
            return self.reject_transaction(effect_builder, *event_metadata, error);
        }

        if !package.is_version_enabled(entity_version_key) {
            let error = Error::parameter_failure(
                &block_header,
                ParameterFailure::DisabledEntityAtVersion { entity_version },
            );
            return self.reject_transaction(effect_builder, *event_metadata, error);
        }

        match package.lookup_entity_hash(entity_version_key) {
            Some(&contract_hash) => {
                let entity_addr = EntityAddr::SmartContract(contract_hash.value());
                effect_builder
                    .get_addressable_entity(*block_header.state_root_hash(), entity_addr)
                    .event(move |result| Event::GetContractResult {
                        event_metadata,
                        block_header,
                        is_payment,
                        contract_hash,
                        maybe_entity: result.into_option(),
                    })
            }
            None => {
                let error = Error::parameter_failure(
                    &block_header,
                    ParameterFailure::InvalidEntityAtVersion { entity_version },
                );
                self.reject_transaction(effect_builder, *event_metadata, error)
            }
        }
    }

    fn validate_transaction_cryptography<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
    ) -> Effects<Event> {
        let is_valid = match &event_metadata.transaction {
            Transaction::Deploy(deploy) => deploy
                .is_valid()
                .map_err(|err| Error::InvalidTransaction(err.into())),
            Transaction::V1(txn) => txn
                .verify()
                .map_err(|err| Error::InvalidTransaction(err.into())),
        };
        if let Err(error) = is_valid {
            return self.reject_transaction(effect_builder, *event_metadata, error);
        }

        // If this has been received from the speculative exec server, we just want to call the
        // responder and finish.  Otherwise store the transaction and announce it if required.
        if let Source::SpeculativeExec = event_metadata.source {
            if let Some(responder) = event_metadata.maybe_responder {
                return responder.respond(Ok(())).ignore();
            }
            error!("speculative exec source should always have a responder");
            return Effects::new();
        }

        effect_builder
            .put_transaction_to_storage(event_metadata.transaction.clone())
            .event(move |is_new| Event::PutToStorageResult {
                event_metadata,
                is_new,
            })
    }

    fn reject_transaction<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: EventMetadata,
        error: Error,
    ) -> Effects<Event> {
        debug!(%error, transaction = %event_metadata.transaction, "rejected transaction");
        let EventMetadata {
            transaction,
            source,
            maybe_responder,
            verification_start_timestamp,
        } = event_metadata;
        self.metrics.observe_rejected(verification_start_timestamp);
        let mut effects = Effects::new();
        if let Some(responder) = maybe_responder {
            // The client has submitted an invalid transaction
            // Return an error to the RPC component via the responder.
            effects.extend(responder.respond(Err(error)).ignore());
        }

        effects.extend(
            effect_builder
                .announce_invalid_transaction(transaction, source)
                .ignore(),
        );
        effects
    }

    fn handle_put_to_storage<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        is_new: bool,
    ) -> Effects<Event> {
        let mut effects = Effects::new();
        if is_new {
            debug!(transaction = %event_metadata.transaction, "accepted transaction");
            effects.extend(
                effect_builder
                    .announce_new_transaction_accepted(
                        Arc::new(event_metadata.transaction),
                        event_metadata.source,
                    )
                    .ignore(),
            );
        } else if matches!(event_metadata.source, Source::Peer(_)) {
            // If `is_new` is `false`, the transaction was previously stored.  If the source is
            // `Peer`, we got here as a result of a `Fetch<Deploy>` or `Fetch<TransactionV1>`, and
            // the incoming transaction could have a different set of approvals to the one already
            // stored.  We can treat the incoming approvals as finalized and now try and store them.
            // If storing them returns `true`, (indicating the approvals are different to any
            // previously stored) we can announce a new transaction accepted, causing the fetcher
            // to be notified.
            return effect_builder
                .store_finalized_approvals(
                    event_metadata.transaction.hash(),
                    event_metadata.transaction.approvals(),
                )
                .event(move |is_new| Event::StoredFinalizedApprovals {
                    event_metadata,
                    is_new,
                });
        }
        self.metrics
            .observe_accepted(event_metadata.verification_start_timestamp);

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
    ) -> Effects<Event> {
        let EventMetadata {
            transaction,
            source,
            maybe_responder,
            verification_start_timestamp,
        } = *event_metadata;
        debug!(%transaction, "accepted transaction");
        self.metrics.observe_accepted(verification_start_timestamp);
        let mut effects = Effects::new();
        if is_new {
            effects.extend(
                effect_builder
                    .announce_new_transaction_accepted(Arc::new(transaction), source)
                    .ignore(),
            );
        }

        if let Some(responder) = maybe_responder {
            effects.extend(responder.respond(Ok(())).ignore());
        }
        effects
    }
}

impl<REv: ReactorEventT> Component<REv> for TransactionAcceptor {
    type Event = Event;

    fn name(&self) -> &str {
        COMPONENT_NAME
    }

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!(?event, "TransactionAcceptor: handling event");
        match event {
            Event::Accept {
                transaction,
                source,
                maybe_responder: responder,
            } => self.accept(effect_builder, transaction, source, responder),
            Event::GetBlockHeaderResult {
                event_metadata,
                maybe_block_header,
            } => self.handle_get_block_header_result(
                effect_builder,
                event_metadata,
                maybe_block_header,
            ),
            Event::GetAddressableEntityResult {
                event_metadata,
                block_header,
                maybe_entity,
            } => self.handle_get_entity_result(
                effect_builder,
                event_metadata,
                block_header,
                maybe_entity,
            ),
            Event::GetBalanceResult {
                event_metadata,
                block_header,
                maybe_balance,
            } => self.handle_get_balance_result(
                effect_builder,
                event_metadata,
                block_header,
                maybe_balance,
            ),
            Event::GetContractResult {
                event_metadata,
                block_header,
                is_payment,
                contract_hash,
                maybe_entity,
            } => self.handle_get_contract_result(
                effect_builder,
                event_metadata,
                block_header,
                is_payment,
                contract_hash,
                maybe_entity,
            ),
            Event::GetPackageResult {
                event_metadata,
                block_header,
                is_payment,
                package_hash,
                maybe_package_version,
                maybe_package,
            } => self.handle_get_package_result(
                effect_builder,
                event_metadata,
                block_header,
                is_payment,
                package_hash,
                maybe_package_version,
                maybe_package,
            ),
            Event::GetEntryPointResult {
                event_metadata,
                block_header,
                is_payment,
                entry_point_name,
                maybe_entry_point,
            } => self.handle_get_entry_point_result(
                effect_builder,
                event_metadata,
                block_header,
                is_payment,
                entry_point_name,
                maybe_entry_point,
            ),
            Event::PutToStorageResult {
                event_metadata,
                is_new,
            } => self.handle_put_to_storage(effect_builder, event_metadata, is_new),
            Event::StoredFinalizedApprovals {
                event_metadata,
                is_new,
            } => self.handle_stored_finalized_approvals(effect_builder, event_metadata, is_new),
        }
    }
}

// `allow` can be removed once https://github.com/casper-network/casper-node/issues/3063 is fixed.
#[allow(clippy::result_large_err)]
fn is_authorized_entity(
    addressable_entity: &AddressableEntity,
    administrators: &BTreeSet<AccountHash>,
    event_metadata: &EventMetadata,
) -> Result<(), ParameterFailure> {
    let authorization_keys = event_metadata.transaction.signers();

    if administrators
        .intersection(&authorization_keys)
        .next()
        .is_some()
    {
        return Ok(());
    }

    if !addressable_entity.can_authorize(&authorization_keys) {
        return Err(ParameterFailure::InvalidAssociatedKeys);
    }

    if !addressable_entity.can_deploy_with(&authorization_keys) {
        return Err(ParameterFailure::InsufficientSignatureWeight);
    }

    Ok(())
}

// `allow` can be removed once https://github.com/casper-network/casper-node/issues/3063 is fixed.
#[allow(clippy::result_large_err)]
fn deploy_payment_is_valid(
    payment: &ExecutableDeployItem,
    block_header: &BlockHeader,
) -> Result<(), Error> {
    match payment {
        ExecutableDeployItem::Transfer { .. } => {
            return Err(Error::parameter_failure(
                block_header,
                DeployParameterFailure::InvalidPaymentVariant.into(),
            ));
        }
        ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
            // module bytes being empty implies the payment executable is standard payment.
            if module_bytes.is_empty() {
                if let Some(value) = args.get(ARG_AMOUNT) {
                    if value.to_t::<U512>().is_err() {
                        return Err(Error::parameter_failure(
                            block_header,
                            DeployParameterFailure::FailedToParsePaymentAmount.into(),
                        ));
                    }
                } else {
                    return Err(Error::parameter_failure(
                        block_header,
                        DeployParameterFailure::MissingPaymentAmount.into(),
                    ));
                }
            }
        }
        ExecutableDeployItem::StoredContractByHash { .. }
        | ExecutableDeployItem::StoredContractByName { .. }
        | ExecutableDeployItem::StoredVersionedContractByHash { .. }
        | ExecutableDeployItem::StoredVersionedContractByName { .. } => (),
    }
    Ok(())
}
