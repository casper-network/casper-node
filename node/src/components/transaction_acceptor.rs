mod config;
mod error;
mod event;
mod metrics;
mod tests;

use std::{collections::BTreeSet, fmt::Debug, sync::Arc};

use casper_types::{
    contracts::ProtocolVersionMajor, ContractRuntimeTag, InvalidTransaction, InvalidTransactionV1,
};
use datasize::DataSize;
use prometheus::Registry;
use tracing::{debug, error, trace};

use casper_storage::data_access_layer::{
    balance::BalanceHandling, BalanceRequest, ProofHandling, QueryRequest, QueryResult,
};
use casper_types::{
    account::AccountHash, addressable_entity::AddressableEntity, evm, system::auction::ARG_AMOUNT,
    AddressableEntityHash, AddressableEntityIdentifier, BlockHeader, CLType, Chainspec, EntityAddr,
    EntityKind, EntityVersion, EntityVersionKey, ExecutableDeployItem,
    ExecutableDeployItemIdentifier, Key, Package, PackageAddr, PackageHash, PackageIdentifier,
    StoredValue, Timestamp, Transaction, TransactionEntryPoint, TransactionInvocationTarget,
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
    types::MetaTransaction,
    utils::Source,
    NodeRng,
};

pub(crate) use config::Config;
pub(crate) use error::{DeployParameterFailure, Error, ParameterFailure};
pub(crate) use event::{
    Event, EventMetadata, EvmAccountLookup, EvmBalanceSource, EvmCodeHashLookup, EvmNonceLookup,
};

const COMPONENT_NAME: &str = "transaction_acceptor";

const ARG_TARGET: &str = "target";

fn evm_account_lookup_from_query_result(query_result: QueryResult) -> EvmAccountLookup {
    match query_result {
        QueryResult::Success { value, .. } => match *value {
            StoredValue::CLValue(cl_value) => match cl_value.into_t::<Key>() {
                Ok(Key::Account(account_hash)) => EvmAccountLookup::Account(account_hash),
                Ok(Key::URef(uref)) => EvmAccountLookup::Purse(uref),
                Ok(other) => {
                    EvmAccountLookup::Invalid(format!("invalid EVM account identity key: {other}"))
                }
                Err(error) => EvmAccountLookup::Invalid(format!(
                    "failed to decode EVM account identity key: {error}"
                )),
            },
            stored_value => EvmAccountLookup::Invalid(format!(
                "expected StoredValue::CLValue(Key), found {}",
                stored_value.type_name()
            )),
        },
        QueryResult::RootNotFound | QueryResult::ValueNotFound(_) | QueryResult::Failure(_) => {
            EvmAccountLookup::Missing
        }
    }
}

fn evm_nonce_from_query_result(query_result: QueryResult) -> EvmNonceLookup {
    match query_result {
        QueryResult::Success { value, .. } => match *value {
            StoredValue::CLValue(cl_value) => match cl_value.into_t::<u64>() {
                Ok(nonce) => EvmNonceLookup::Value(nonce),
                Err(error) => {
                    EvmNonceLookup::Invalid(format!("failed to decode EVM nonce: {error}"))
                }
            },
            stored_value => EvmNonceLookup::Invalid(format!(
                "expected StoredValue::CLValue(u64), found {}",
                stored_value.type_name()
            )),
        },
        QueryResult::RootNotFound | QueryResult::ValueNotFound(_) | QueryResult::Failure(_) => {
            EvmNonceLookup::Missing
        }
    }
}

fn evm_code_hash_from_query_result(query_result: QueryResult) -> EvmCodeHashLookup {
    match query_result {
        QueryResult::Success { value, .. } => match *value {
            StoredValue::CLValue(cl_value) => match cl_value.into_t::<evm::Hash>() {
                Ok(code_hash) => EvmCodeHashLookup::Value(code_hash),
                Err(error) => {
                    EvmCodeHashLookup::Invalid(format!("failed to decode EVM code hash: {error}"))
                }
            },
            stored_value => EvmCodeHashLookup::Invalid(format!(
                "expected StoredValue::CLValue(evm::Hash), found {}",
                stored_value.type_name()
            )),
        },
        QueryResult::RootNotFound | QueryResult::ValueNotFound(_) | QueryResult::Failure(_) => {
            EvmCodeHashLookup::Missing
        }
    }
}

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
        input_transaction: Transaction,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    ) -> Effects<Event> {
        trace!(%source, %input_transaction, "checking transaction before accepting");
        let verification_start_timestamp = Timestamp::now();
        let transaction_config = &self.chainspec.as_ref().transaction_config;
        let maybe_meta_transaction = MetaTransaction::from_transaction(
            &input_transaction,
            self.chainspec.as_ref().core_config.pricing_handling,
            transaction_config,
        );
        let meta_transaction = match maybe_meta_transaction {
            Ok(transaction) => transaction,
            Err(err) => {
                return self.reject_transaction_direct(
                    effect_builder,
                    input_transaction,
                    source,
                    maybe_responder,
                    verification_start_timestamp,
                    Error::InvalidTransaction(err),
                );
            }
        };

        let event_metadata = Box::new(EventMetadata::new(
            input_transaction,
            meta_transaction.clone(),
            source,
            maybe_responder,
            verification_start_timestamp,
        ));

        if meta_transaction.is_install_or_upgrade()
            && meta_transaction.is_v2_wasm()
            && meta_transaction.seed().is_none()
        {
            return self.reject_transaction(
                effect_builder,
                *event_metadata,
                Error::InvalidTransaction(InvalidTransaction::V1(
                    InvalidTransactionV1::MissingSeed,
                )),
            );
        }

        let is_config_compliant = event_metadata
            .meta_transaction
            .is_config_compliant(
                &self.chainspec,
                self.acceptor_config.timestamp_leeway,
                verification_start_timestamp,
            )
            .map_err(Error::InvalidTransaction);

        if let Err(error) = is_config_compliant {
            return self.reject_transaction(effect_builder, *event_metadata, error);
        }

        if event_metadata
            .meta_transaction
            .as_evm()
            .is_some_and(|evm_transaction| evm_transaction.is_unsigned_call())
        {
            return self.reject_transaction(
                effect_builder,
                *event_metadata,
                Error::InvalidTransaction(InvalidTransaction::Evm(
                    evm::TransactionError::MissingApproval,
                )),
            );
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

        if let Some(evm_transaction) = event_metadata.meta_transaction.as_evm() {
            // EVM senders are validated from the EVM identity record first. A
            // Casper account lookup would be wrong here because an EVM address
            // may be either linked to a Casper account or backed by an
            // EVM-native purse.
            let query_request = QueryRequest::new(
                *block_header.state_root_hash(),
                Key::Evm(evm::EvmAddr::Account(evm_transaction.from())),
                vec![],
            );
            return effect_builder
                .query_global_state(query_request)
                .event(move |query_result| Event::GetEvmAccountResult {
                    event_metadata,
                    block_header,
                    account: evm_account_lookup_from_query_result(query_result),
                });
        }

        if event_metadata.source.is_client() {
            let initiator_addr = event_metadata.transaction.initiator_addr();
            let account_hash = initiator_addr
                .account_hash()
                .expect("non-EVM transaction initiator must be a Casper account");
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

    fn handle_get_evm_account_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        account: EvmAccountLookup,
    ) -> Effects<Event> {
        let evm_transaction = event_metadata
            .meta_transaction
            .as_evm()
            .expect("EVM account lookup should only be used for EVM transactions");

        match account {
            // Existing identity pointers select the balance source directly.
            // The nonce remains under `EvmAddr::Nonce`, so it is queried after
            // identity resolution regardless of whether the payer is a Casper
            // account or an EVM-native purse.
            EvmAccountLookup::Account(account_hash) => self.query_evm_nonce(
                effect_builder,
                event_metadata,
                block_header,
                EvmBalanceSource::Account(account_hash),
            ),
            EvmAccountLookup::Purse(uref) => self.query_evm_nonce(
                effect_builder,
                event_metadata,
                block_header,
                EvmBalanceSource::Purse(uref),
            ),
            EvmAccountLookup::Invalid(error_message) => {
                let error = Error::InvalidTransaction(InvalidTransaction::Evm(
                    evm::TransactionError::Decode(error_message),
                ));
                self.reject_transaction(effect_builder, *event_metadata, error)
            }
            EvmAccountLookup::Missing => {
                // A missing identity pointer does not necessarily mean all EVM
                // metadata is missing. Runtime still checks split nonce and
                // code-hash records before deciding whether the address can be
                // linked to a Casper account or must remain EVM-native.
                let query_request = QueryRequest::new(
                    *block_header.state_root_hash(),
                    Key::Evm(evm::EvmAddr::Nonce(evm_transaction.from())),
                    vec![],
                );
                effect_builder
                    .query_global_state(query_request)
                    .event(
                        move |query_result| Event::GetMissingEvmIdentityNonceResult {
                            event_metadata,
                            block_header,
                            nonce: evm_nonce_from_query_result(query_result),
                        },
                    )
            }
        }
    }

    fn handle_get_missing_evm_identity_nonce_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        nonce: EvmNonceLookup,
    ) -> Effects<Event> {
        let evm_transaction = event_metadata
            .meta_transaction
            .as_evm()
            .expect("missing EVM identity nonce lookup should only be used for EVM transactions");
        let expected_nonce = match nonce {
            EvmNonceLookup::Value(nonce) => nonce,
            EvmNonceLookup::Missing => 0,
            EvmNonceLookup::Invalid(error_message) => {
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::InvalidTransaction(InvalidTransaction::Evm(
                        evm::TransactionError::Decode(error_message),
                    )),
                );
            }
        };
        let query_request = QueryRequest::new(
            *block_header.state_root_hash(),
            Key::Evm(evm::EvmAddr::CodeHash(evm_transaction.from())),
            vec![],
        );
        effect_builder
            .query_global_state(query_request)
            .event(
                move |query_result| Event::GetMissingEvmIdentityCodeHashResult {
                    event_metadata,
                    block_header,
                    expected_nonce,
                    code_hash: evm_code_hash_from_query_result(query_result),
                },
            )
    }

    fn handle_get_missing_evm_identity_code_hash_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        expected_nonce: u64,
        code_hash: EvmCodeHashLookup,
    ) -> Effects<Event> {
        let evm_transaction = event_metadata.meta_transaction.as_evm().expect(
            "missing EVM identity code-hash lookup should only be used for EVM transactions",
        );
        let address = evm_transaction.from();
        let code_hash = match code_hash {
            EvmCodeHashLookup::Value(code_hash) => code_hash,
            EvmCodeHashLookup::Missing => evm::EMPTY_CODE_HASH,
            EvmCodeHashLookup::Invalid(error_message) => {
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::InvalidTransaction(InvalidTransaction::Evm(
                        evm::TransactionError::Decode(error_message),
                    )),
                );
            }
        };

        if code_hash != evm::EMPTY_CODE_HASH {
            return self.validate_evm_nonce_and_balance(
                effect_builder,
                event_metadata,
                block_header,
                expected_nonce,
                EvmBalanceSource::Purse(evm::deterministic_purse(address)),
            );
        }

        let account_hash = match evm_transaction.signer() {
            Ok(signer) => signer.to_account_hash(),
            Err(error) => {
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::InvalidTransaction(InvalidTransaction::Evm(error)),
                );
            }
        };
        let entity_addr = EntityAddr::Account(account_hash.value());
        // If the recovered signer already has a Casper account, client balance
        // validation should use that account. Otherwise it uses the
        // deterministic EVM purse, matching the account-creation plan runtime
        // will apply only after payment preconditions pass.
        effect_builder
            .get_addressable_entity(*block_header.state_root_hash(), entity_addr)
            .event(move |result| Event::GetEvmAccountEntityResult {
                event_metadata,
                block_header,
                expected_nonce,
                account_hash,
                maybe_entity: result.into_option(),
            })
    }

    fn handle_get_evm_account_entity_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        expected_nonce: u64,
        account_hash: AccountHash,
        maybe_entity: Option<AddressableEntity>,
    ) -> Effects<Event> {
        let evm_transaction = event_metadata
            .meta_transaction
            .as_evm()
            .expect("EVM account entity lookup should only be used for EVM transactions");
        // This is still a read-only acceptor decision. It does not create the
        // Casper account or write `EvmAddr::Account`; it only picks the balance
        // source that runtime will use when it evaluates the same origin.
        let balance_source = if maybe_entity.is_some() {
            EvmBalanceSource::Account(account_hash)
        } else {
            EvmBalanceSource::Purse(evm::deterministic_purse(evm_transaction.from()))
        };
        self.validate_evm_nonce_and_balance(
            effect_builder,
            event_metadata,
            block_header,
            expected_nonce,
            balance_source,
        )
    }

    fn query_evm_nonce<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        balance_source: EvmBalanceSource,
    ) -> Effects<Event> {
        let evm_transaction = event_metadata
            .meta_transaction
            .as_evm()
            .expect("EVM nonce lookup should only be used for EVM transactions");
        let query_request = QueryRequest::new(
            *block_header.state_root_hash(),
            Key::Evm(evm::EvmAddr::Nonce(evm_transaction.from())),
            vec![],
        );
        effect_builder
            .query_global_state(query_request)
            .event(move |query_result| Event::GetEvmNonceResult {
                event_metadata,
                block_header,
                balance_source,
                nonce: evm_nonce_from_query_result(query_result),
            })
    }

    fn handle_get_evm_nonce_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        balance_source: EvmBalanceSource,
        nonce: EvmNonceLookup,
    ) -> Effects<Event> {
        let expected_nonce = match nonce {
            EvmNonceLookup::Value(nonce) => nonce,
            EvmNonceLookup::Missing => 0,
            EvmNonceLookup::Invalid(error_message) => {
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::InvalidTransaction(InvalidTransaction::Evm(
                        evm::TransactionError::Decode(error_message),
                    )),
                );
            }
        };
        self.validate_evm_nonce_and_balance(
            effect_builder,
            event_metadata,
            block_header,
            expected_nonce,
            balance_source,
        )
    }

    fn validate_evm_nonce_and_balance<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        expected: u64,
        balance_source: EvmBalanceSource,
    ) -> Effects<Event> {
        let evm_transaction = event_metadata
            .meta_transaction
            .as_evm()
            .expect("EVM account validation should only be used for EVM transactions");
        let actual = evm_transaction.nonce();
        // EVM nonce validation is independent of the identity pointer. Linking
        // an EVM address to a Casper account does not change the EVM replay
        // counter.
        if actual != expected {
            return self.reject_transaction(
                effect_builder,
                *event_metadata,
                Error::InvalidTransaction(InvalidTransaction::Evm(
                    evm::TransactionError::InvalidNonce { expected, actual },
                )),
            );
        }

        if event_metadata.source.is_client() {
            let balance_request = match balance_source {
                EvmBalanceSource::Purse(main_purse) => BalanceRequest::from_purse(
                    *block_header.state_root_hash(),
                    block_header.protocol_version(),
                    main_purse,
                    BalanceHandling::Available,
                    ProofHandling::NoProofs,
                ),
                EvmBalanceSource::Account(account_hash) => BalanceRequest::from_account_hash(
                    *block_header.state_root_hash(),
                    block_header.protocol_version(),
                    account_hash,
                    BalanceHandling::Available,
                    ProofHandling::NoProofs,
                ),
            };
            effect_builder
                .get_balance(balance_request)
                .event(move |balance_result| Event::GetBalanceResult {
                    event_metadata,
                    block_header,
                    maybe_balance: balance_result.available_balance().copied(),
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
                let has_minimum_balance =
                    balance >= self.chainspec.core_config.baseline_motes_amount_u512();
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
            | ExecutableDeployItemIdentifier::Package(PackageIdentifier::Name { .. })
            | ExecutableDeployItemIdentifier::Package(PackageIdentifier::NameWithMajorVersion {
                ..
            }) => self.verify_body(effect_builder, event_metadata, block_header),
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
            )
            | ExecutableDeployItemIdentifier::Package(
                ref contract_package_identifier @ PackageIdentifier::HashWithMajorVersion {
                    package_hash,
                    ..
                },
            ) => {
                let maybe_entity_version = contract_package_identifier.version();
                let maybe_protocol_version_major =
                    contract_package_identifier.protocol_version_major();
                effect_builder
                    .get_package(*block_header.state_root_hash(), package_hash.value())
                    .event(move |maybe_package| Event::GetPackageResult {
                        event_metadata,
                        block_header,
                        is_payment: true,
                        package_hash,
                        maybe_entity_version,
                        maybe_protocol_version_major,
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
        match &event_metadata.meta_transaction {
            MetaTransaction::Deploy(_) => {
                self.verify_deploy_session(effect_builder, event_metadata, block_header)
            }
            MetaTransaction::Evm(_) => {
                self.validate_transaction_cryptography(effect_builder, event_metadata)
            }
            MetaTransaction::V1(_) => {
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
        let session = match &event_metadata.meta_transaction {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy.session(),
            MetaTransaction::Evm(_) => {
                error!("should only handle deploys in verify_deploy_session");
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::ExpectedDeploy,
                );
            }
            MetaTransaction::V1(txn) => {
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
                let Some(target) = args.get(ARG_TARGET) else {
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::MissingTransferTarget.into(),
                    );
                    return self.reject_transaction(effect_builder, *event_metadata, error);
                };
                if !self.chainspec.evm_config.enabled
                    && target.cl_type() == &CLType::ByteArray(evm::ADDRESS_LENGTH as u32)
                {
                    let error = Error::parameter_failure(
                        &block_header,
                        DeployParameterFailure::EvmAddressTransferDisabled.into(),
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
            | ExecutableDeployItemIdentifier::Package(PackageIdentifier::Name { .. })
            | ExecutableDeployItemIdentifier::Package(PackageIdentifier::NameWithMajorVersion {
                ..
            }) => self.validate_transaction_cryptography(effect_builder, event_metadata),
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
            )
            | ExecutableDeployItemIdentifier::Package(
                ref package_identifier @ PackageIdentifier::HashWithMajorVersion {
                    package_hash, ..
                },
            ) => {
                let maybe_package_version = package_identifier.version();
                effect_builder
                    .get_package(*block_header.state_root_hash(), package_hash.value())
                    .event(move |maybe_package| Event::GetPackageResult {
                        event_metadata,
                        block_header,
                        is_payment: false,
                        package_hash,
                        maybe_entity_version: maybe_package_version,
                        maybe_protocol_version_major: None,
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
            GetPackage(
                PackageAddr,
                Option<EntityVersion>,
                Option<ProtocolVersionMajor>,
            ),
            CryptoValidation,
        }

        let next_step = match &event_metadata.meta_transaction {
            MetaTransaction::Deploy(meta_deploy) => {
                let deploy_hash = meta_deploy.deploy().hash();
                error!(
                    %deploy_hash,
                    "should only handle version 1 transactions in verify_transaction_v1_body"
                );
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::ExpectedTransactionV1,
                );
            }
            MetaTransaction::Evm(_) => {
                error!("should only handle version 1 transactions in verify_transaction_v1_body");
                return self.reject_transaction(
                    effect_builder,
                    *event_metadata,
                    Error::ExpectedTransactionV1,
                );
            }
            MetaTransaction::V1(txn) => match txn.target() {
                TransactionTarget::Stored { id, .. } => match id {
                    TransactionInvocationTarget::ByHash(entity_addr) => {
                        NextStep::GetContract(EntityAddr::SmartContract(*entity_addr))
                    }
                    TransactionInvocationTarget::ByPackageHash {
                        addr,
                        version,
                        protocol_version_major,
                    } => NextStep::GetPackage(*addr, *version, *protocol_version_major),
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
            NextStep::GetPackage(
                package_addr,
                maybe_entity_version,
                maybe_protocol_version_major,
            ) => effect_builder
                .get_package(*block_header.state_root_hash(), package_addr)
                .event(move |maybe_package| Event::GetPackageResult {
                    event_metadata,
                    block_header,
                    is_payment: false,
                    package_hash: PackageHash::new(package_addr),
                    maybe_entity_version,
                    maybe_protocol_version_major,
                    maybe_package,
                }),
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
        let addressable_entity = match maybe_contract {
            Some(addressable_entity) => addressable_entity,
            None => {
                let error = Error::parameter_failure(
                    &block_header,
                    ParameterFailure::NoSuchContractAtHash { contract_hash },
                );
                return self.reject_transaction(effect_builder, *event_metadata, error);
            }
        };

        let maybe_entry_point_name = match &event_metadata.meta_transaction {
            MetaTransaction::Deploy(meta_deploy) if is_payment => Some(
                meta_deploy
                    .deploy()
                    .payment()
                    .entry_point_name()
                    .to_string(),
            ),
            MetaTransaction::Deploy(meta_deploy) => Some(
                meta_deploy
                    .deploy()
                    .session()
                    .entry_point_name()
                    .to_string(),
            ),
            MetaTransaction::Evm(_) => {
                error!("should not fetch a contract to validate EVM transactions");
                None
            }
            MetaTransaction::V1(_) if is_payment => {
                error!("should not fetch a contract to validate payment logic for transaction v1s");
                None
            }
            MetaTransaction::V1(txn) => match txn.entry_point() {
                TransactionEntryPoint::Call => Some(DEFAULT_ENTRY_POINT_NAME.to_owned()),
                TransactionEntryPoint::Custom(name) => Some(name.clone()),
                TransactionEntryPoint::Transfer
                | TransactionEntryPoint::Burn
                | TransactionEntryPoint::AddBid
                | TransactionEntryPoint::WithdrawBid
                | TransactionEntryPoint::Delegate
                | TransactionEntryPoint::Undelegate
                | TransactionEntryPoint::Redelegate
                | TransactionEntryPoint::ActivateBid
                | TransactionEntryPoint::ChangeBidPublicKey
                | TransactionEntryPoint::AddReservations
                | TransactionEntryPoint::CancelReservations => None,
            },
        };

        match maybe_entry_point_name {
            Some(entry_point_name) => effect_builder
                .does_entry_point_exist(
                    *block_header.state_root_hash(),
                    contract_hash.value(),
                    entry_point_name.clone(),
                )
                .event(move |entry_point_result| Event::GetEntryPointResult {
                    event_metadata,
                    block_header,
                    is_payment,
                    entry_point_name,
                    addressable_entity,
                    entry_point_exists: entry_point_result.is_success(),
                }),

            None => {
                if is_payment {
                    return self.verify_body(effect_builder, event_metadata, block_header);
                }
                self.validate_transaction_cryptography(effect_builder, event_metadata)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_get_entry_point_result<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
        block_header: Box<BlockHeader>,
        is_payment: bool,
        entry_point_name: String,
        addressable_entity: AddressableEntity,
        entry_point_exist: bool,
    ) -> Effects<Event> {
        match addressable_entity.kind() {
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1)
            | EntityKind::Account(_)
            | EntityKind::System(_) => {
                if !entry_point_exist {
                    let error = Error::parameter_failure(
                        &block_header,
                        ParameterFailure::NoSuchEntryPoint { entry_point_name },
                    );
                    return self.reject_transaction(effect_builder, *event_metadata, error);
                }
                if is_payment {
                    return self.verify_body(effect_builder, event_metadata, block_header);
                }
                self.validate_transaction_cryptography(effect_builder, event_metadata)
            }
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV2) => {
                // Engine V2 does not store entrypoint information on chain and relies entirely on
                // the Wasm itself.
                self.validate_transaction_cryptography(effect_builder, event_metadata)
            }
        }
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
        maybe_protocol_version_major: Option<ProtocolVersionMajor>,
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

        let maybe_entity_version_key = match self.resolve_entity_version_key(
            package.as_ref(),
            maybe_contract_version,
            maybe_protocol_version_major,
            &block_header,
        ) {
            Ok(maybe) => maybe,
            Err(err) => return self.reject_transaction(effect_builder, *event_metadata, *err),
        };
        let entity_version_key = match maybe_entity_version_key {
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

        if package.is_version_missing(entity_version_key) {
            let error = Error::parameter_failure(
                &block_header,
                ParameterFailure::MissingEntityAtVersion { entity_version_key },
            );
            return self.reject_transaction(effect_builder, *event_metadata, error);
        }

        if !package.is_version_enabled(entity_version_key) {
            let error = Error::parameter_failure(
                &block_header,
                ParameterFailure::DisabledEntityAtVersion { entity_version_key },
            );
            return self.reject_transaction(effect_builder, *event_metadata, error);
        }

        match package.lookup_entity_hash(entity_version_key) {
            Some(&entity_addr) => {
                let contract_hash = AddressableEntityHash::new(entity_addr.value());
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
                    ParameterFailure::InvalidEntityAtVersion { entity_version_key },
                );
                self.reject_transaction(effect_builder, *event_metadata, error)
            }
        }
    }

    /// Resolves EntityVersionKey for a given contract. Returning Some(k) means that k is an enabled
    /// version matching the criteria. Returning None doesn't mean there is no fit - it means
    /// that we can't for sure determine the version key since the state at execution might be
    /// different - we must assume that a valid EntityVersionKey might be present for the package or
    /// error out during execution
    fn resolve_entity_version_key(
        &self,
        package: &Package,
        maybe_entity_version: Option<EntityVersion>,
        maybe_protocol_version_major: Option<ProtocolVersionMajor>,
        block_header: &BlockHeader,
    ) -> Result<Option<EntityVersionKey>, Box<Error>> {
        let entity_version_key = match (maybe_entity_version, maybe_protocol_version_major) {
            (Some(entity_version), Some(major)) => EntityVersionKey::new(major, entity_version),
            (Some(_), None) | (None, Some(_)) | (None, None) => return Ok(None), /* In this case
                                                                                  * the runtime
                                                                                  * needs to do
                                                                                  * the
                                                                                  * determination, at this point we can't be sure which versions will be available on execution */
        };

        if package.is_version_missing(entity_version_key) {
            return Err(Box::new(Error::parameter_failure(
                block_header,
                ParameterFailure::MissingEntityAtVersion { entity_version_key },
            )));
        }

        if !package.is_version_enabled(entity_version_key) {
            return Err(Box::new(Error::parameter_failure(
                block_header,
                ParameterFailure::DisabledEntityAtVersion { entity_version_key },
            )));
        }
        Ok(Some(entity_version_key))
    }

    fn validate_transaction_cryptography<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        event_metadata: Box<EventMetadata>,
    ) -> Effects<Event> {
        let is_valid = match &event_metadata.meta_transaction {
            MetaTransaction::Deploy(meta_deploy) => meta_deploy
                .deploy()
                .is_valid()
                .map_err(|err| Error::InvalidTransaction(err.into())),
            MetaTransaction::Evm(evm) => evm
                .verify()
                .map_err(|err| Error::InvalidTransaction(err.into())),
            MetaTransaction::V1(txn) => txn
                .verify()
                .map_err(|err| Error::InvalidTransaction(err.into())),
        };
        if let Err(error) = is_valid {
            return self.reject_transaction(effect_builder, *event_metadata, error);
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
        let EventMetadata {
            meta_transaction: _,
            transaction,
            source,
            maybe_responder,
            verification_start_timestamp,
        } = event_metadata;
        self.reject_transaction_direct(
            effect_builder,
            transaction,
            source,
            maybe_responder,
            verification_start_timestamp,
            error,
        )
    }

    fn reject_transaction_direct<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        transaction: Transaction,
        source: Source,
        maybe_responder: Option<Responder<Result<(), Error>>>,
        verification_start_timestamp: Timestamp,
        error: Error,
    ) -> Effects<Event> {
        trace!(%error, transaction = %transaction, "rejected transaction");
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
            meta_transaction: _,
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
            Event::GetEvmAccountResult {
                event_metadata,
                block_header,
                account,
            } => self.handle_get_evm_account_result(
                effect_builder,
                event_metadata,
                block_header,
                account,
            ),
            Event::GetEvmNonceResult {
                event_metadata,
                block_header,
                balance_source,
                nonce,
            } => self.handle_get_evm_nonce_result(
                effect_builder,
                event_metadata,
                block_header,
                balance_source,
                nonce,
            ),
            Event::GetMissingEvmIdentityNonceResult {
                event_metadata,
                block_header,
                nonce,
            } => self.handle_get_missing_evm_identity_nonce_result(
                effect_builder,
                event_metadata,
                block_header,
                nonce,
            ),
            Event::GetMissingEvmIdentityCodeHashResult {
                event_metadata,
                block_header,
                expected_nonce,
                code_hash,
            } => self.handle_get_missing_evm_identity_code_hash_result(
                effect_builder,
                event_metadata,
                block_header,
                expected_nonce,
                code_hash,
            ),
            Event::GetEvmAccountEntityResult {
                event_metadata,
                block_header,
                expected_nonce,
                account_hash,
                maybe_entity,
            } => self.handle_get_evm_account_entity_result(
                effect_builder,
                event_metadata,
                block_header,
                expected_nonce,
                account_hash,
                maybe_entity,
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
                maybe_entity_version,
                maybe_protocol_version_major,
                maybe_package,
            } => self.handle_get_package_result(
                effect_builder,
                event_metadata,
                block_header,
                is_payment,
                package_hash,
                maybe_entity_version,
                maybe_protocol_version_major,
                maybe_package,
            ),
            Event::GetEntryPointResult {
                event_metadata,
                block_header,
                is_payment,
                entry_point_name,
                addressable_entity,
                entry_point_exists,
            } => self.handle_get_entry_point_result(
                effect_builder,
                event_metadata,
                block_header,
                is_payment,
                entry_point_name,
                addressable_entity,
                entry_point_exists,
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
