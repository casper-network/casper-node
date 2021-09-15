use std::fmt::{self, Display, Formatter};

use serde::Serialize;

use super::Source;
use crate::{
    components::deploy_acceptor::Error,
    effect::{announcements::RpcServerAnnouncement, Responder},
    types::{Block, Deploy, NodeId},
};

use casper_execution_engine::{
    core::engine_state::executable_deploy_item::{ContractIdentifier, ContractPackageIdentifier},
    shared::newtypes::Blake2bHash,
};
use casper_types::{account::Account, Contract, ContractPackage, U512};

/// `DeployAcceptor` events.
#[derive(Debug, Serialize)]
pub(crate) enum Event {
    /// The initiating event to accept a new `Deploy`.
    Accept {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        responder: Option<Responder<Result<(), Error>>>,
    },
    /// The result of the `DeployAcceptor` putting a `Deploy` to the storage component.
    PutToStorageResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        is_new: bool,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    /// The InvalidDeployResult event
    InvalidDeployResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        error: Error,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    GetBlockResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_block: Box<Option<Block>>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    GetAccountResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        maybe_account: Option<Account>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    GetBalanceResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        maybe_balance_value: Option<U512>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    VerifyPaymentLogic {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    VerifySessionLogic {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    GetContractResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        is_payment: bool,
        contract_identifier: ContractIdentifier,
        maybe_contract: Option<Contract>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    GetContractPackageResult {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        prestate_hash: Blake2bHash,
        is_payment: bool,
        contract_package_identifier: ContractPackageIdentifier,
        maybe_contract_package: Option<ContractPackage>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
    VerifyDeployCryptographicValidity {
        deploy: Box<Deploy>,
        source: Source<NodeId>,
        maybe_responder: Option<Responder<Result<(), Error>>>,
    },
}

impl From<RpcServerAnnouncement> for Event {
    fn from(announcement: RpcServerAnnouncement) -> Self {
        match announcement {
            RpcServerAnnouncement::DeployReceived { deploy, responder } => Event::Accept {
                deploy,
                source: Source::<NodeId>::Client,
                responder,
            },
        }
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Accept { deploy, source, .. } => {
                write!(formatter, "accept {} from {}", deploy.id(), source)
            }
            Event::PutToStorageResult { deploy, is_new, .. } => {
                if *is_new {
                    write!(formatter, "put new {} to storage", deploy.id())
                } else {
                    write!(formatter, "had already stored {}", deploy.id())
                }
            }

            Event::InvalidDeployResult {
                deploy,
                source,
                error,
                ..
            } => {
                write!(
                    formatter,
                    "deploy received by {} with hash: {} is invalid due to error: {:?}.",
                    source,
                    deploy.id(),
                    error
                )
            }
            Event::GetBlockResult { deploy, .. } => {
                write!(
                    formatter,
                    "received highest block from storage to validate deploy with hash: {}.",
                    deploy.id()
                )
            }
            Event::GetAccountResult { deploy, .. } => {
                write!(
                    formatter,
                    "verifying account to validate deploy with hash {}.",
                    deploy.id()
                )
            }
            Event::GetBalanceResult { deploy, .. } => {
                write!(
                    formatter,
                    "verifying account balance to validate deploy with hash {}.",
                    deploy.id()
                )
            }
            Event::VerifyPaymentLogic {
                deploy,
                prestate_hash,
                ..
            } => {
                write!(
                    formatter,
                    "verifying payment logic to validate deploy with hash {} with state hash: {}.",
                    deploy.id(),
                    prestate_hash
                )
            }
            Event::VerifySessionLogic {
                deploy,
                prestate_hash,
                ..
            } => {
                write!(
                    formatter,
                    "verifying session logic to validate deploy with hash {} with state hash: {}.",
                    deploy.id(),
                    prestate_hash
                )
            }
            Event::GetContractResult {
                deploy,
                prestate_hash,
                ..
            } => {
                write!(
                    formatter,
                    "verifying contract to validate deploy with hash {} with state hash: {}.",
                    deploy.id(),
                    prestate_hash
                )
            }
            Event::GetContractPackageResult {
                deploy,
                prestate_hash,
                ..
            } => {
                write!(
                    formatter,
                    "verifying contract package to validate deploy with hash {} with state hash: {}.",
                    deploy.id(),
                    prestate_hash
                )
            }
            Event::VerifyDeployCryptographicValidity { deploy, .. } => {
                write!(
                    formatter,
                    "verifying deploy cryptographic validity for deploy with hash {}.",
                    deploy.id(),
                )
            }
        }
    }
}
