use std::{collections::BTreeSet, convert::TryFrom};

use serde::Serialize;
use thiserror::Error;

use casper_storage::{data_access_layer::TransferResult, tracking_copy::TrackingCopyCache};
use casper_types::{
    account::AccountHash,
    bytesrepr::Bytes,
    contract_messages::Messages,
    execution::{Effects, TransformKindV2},
    BlockHash, BlockTime, CLValue, DeployHash, Digest, ExecutableDeployItem, Gas, InitiatorAddr,
    Key, PackageHash, Phase, PricingMode, ProtocolVersion, RuntimeArgs, TransactionEntryPoint,
    TransactionHash, TransactionInvocationTarget, TransactionTarget, TransactionV1Hash, Transfer,
    URefAddr, U512,
};

use crate::engine_state::Error as EngineError;

const DEFAULT_ENTRY_POINT: &str = "call";

/// Structure that needs to be filled with data so the engine can assemble wasm for deploy.
pub struct SessionDataDeploy<'a> {
    deploy_hash: &'a DeployHash,
    session: &'a ExecutableDeployItem,
    initiator_addr: InitiatorAddr,
    signers: BTreeSet<AccountHash>,
    is_standard_payment: bool,
}

impl<'a> SessionDataDeploy<'a> {
    /// Constructor
    pub fn new(
        deploy_hash: &'a DeployHash,
        session: &'a ExecutableDeployItem,
        initiator_addr: InitiatorAddr,
        signers: BTreeSet<AccountHash>,
        is_standard_payment: bool,
    ) -> Self {
        Self {
            deploy_hash,
            session,
            initiator_addr,
            signers,
            is_standard_payment,
        }
    }

    /// Deploy hash of the deploy
    pub fn deploy_hash(&self) -> &DeployHash {
        self.deploy_hash
    }

    /// executable item of the deploy
    pub fn session(&self) -> &ExecutableDeployItem {
        self.session
    }

    /// initiator address of the deploy
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    /// signers of the deploy
    pub fn signers(&self) -> BTreeSet<AccountHash> {
        self.signers.clone()
    }
}

/// Structure that needs to be filled with data so the engine can assemble wasm for v1.
pub struct SessionDataV1<'a> {
    args: &'a RuntimeArgs,
    target: &'a TransactionTarget,
    entry_point: &'a TransactionEntryPoint,
    is_install_upgrade: bool,
    hash: &'a TransactionV1Hash,
    pricing_mode: &'a PricingMode,
    initiator_addr: InitiatorAddr,
    signers: BTreeSet<AccountHash>,
    is_standard_payment: bool,
}

impl<'a> SessionDataV1<'a> {
    #[allow(clippy::too_many_arguments)]
    /// Constructor
    pub fn new(
        args: &'a RuntimeArgs,
        target: &'a TransactionTarget,
        entry_point: &'a TransactionEntryPoint,
        is_install_upgrade: bool,
        hash: &'a TransactionV1Hash,
        pricing_mode: &'a PricingMode,
        initiator_addr: InitiatorAddr,
        signers: BTreeSet<AccountHash>,
        is_standard_payment: bool,
    ) -> Self {
        Self {
            args,
            target,
            entry_point,
            is_install_upgrade,
            hash,
            pricing_mode,
            initiator_addr,
            signers,
            is_standard_payment,
        }
    }

    /// Runtime args passed with the transaction.
    pub fn args(&self) -> &RuntimeArgs {
        self.args
    }

    /// Target of the transaction.
    pub fn target(&self) -> &TransactionTarget {
        self.target
    }

    /// Entry point of the transaction
    pub fn entry_point(&self) -> &TransactionEntryPoint {
        self.entry_point
    }

    /// Should session be allowed to perform install/upgrade operations
    pub fn is_install_upgrade(&self) -> bool {
        self.is_install_upgrade
    }

    /// Hash of the transaction
    pub fn hash(&self) -> &TransactionV1Hash {
        self.hash
    }

    /// initiator address of the transaction
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    /// signers of the transaction
    pub fn signers(&self) -> BTreeSet<AccountHash> {
        self.signers.clone()
    }

    /// Pricing mode of the transaction
    pub fn pricing_mode(&self) -> &PricingMode {
        self.pricing_mode
    }
}

/// Wrapper enum abstracting data for assmbling WasmV1Requests
pub enum SessionInputData<'a> {
    /// Variant for sessions created from deploy transactions
    DeploySessionData {
        /// Deploy session data
        data: SessionDataDeploy<'a>,
    },
    /// Variant for sessions created from v1 transactions
    SessionDataV1 {
        /// v1 session data
        data: SessionDataV1<'a>,
    },
}

impl SessionInputData<'_> {
    /// Transaction hash for the session
    pub fn transaction_hash(&self) -> TransactionHash {
        match self {
            SessionInputData::DeploySessionData { data } => {
                TransactionHash::Deploy(*data.deploy_hash())
            }
            SessionInputData::SessionDataV1 { data } => TransactionHash::V1(*data.hash()),
        }
    }

    /// Initiator address for the session
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        match self {
            SessionInputData::DeploySessionData { data } => data.initiator_addr(),
            SessionInputData::SessionDataV1 { data } => data.initiator_addr(),
        }
    }

    /// Signers for the session
    pub fn signers(&self) -> BTreeSet<AccountHash> {
        match self {
            SessionInputData::DeploySessionData { data } => data.signers(),
            SessionInputData::SessionDataV1 { data } => data.signers(),
        }
    }

    /// determines if the transaction from which this session data was created is a standard payment
    pub fn is_standard_payment(&self) -> bool {
        match self {
            SessionInputData::DeploySessionData { data } => data.is_standard_payment,
            SessionInputData::SessionDataV1 { data } => data.is_standard_payment,
        }
    }

    /// Is install upgrade allowed?
    pub fn is_install_upgrade_allowed(&self) -> bool {
        match self {
            SessionInputData::DeploySessionData { .. } => true,
            SessionInputData::SessionDataV1 { data } => data.is_install_upgrade,
        }
    }
}

/// Error returned if constructing a new [`WasmV1Request`] fails.
#[derive(Clone, Eq, PartialEq, Error, Serialize, Debug)]
pub enum InvalidRequest {
    /// Missing custom payment.
    #[error("custom payment not found for {0}")]
    CustomPaymentNotFound(TransactionHash),
    /// Unexpected variant.
    #[error("unexpected variant for {0} attempting {1}")]
    UnexpectedVariant(TransactionHash, String),
    /// Unsupported mode.
    #[error("unsupported mode for {0} attempting {1}")]
    UnsupportedMode(TransactionHash, String),
    /// Invalid entry point.
    #[error("invalid entry point for {0} attempting {1}")]
    InvalidEntryPoint(TransactionHash, String),
    /// Invalid target.
    #[error("invalid target for {0} attempting {1}")]
    InvalidTarget(TransactionHash, String),
    /// Unsupported category.
    #[error("invalid category for {0} attempting {1}")]
    InvalidCategory(TransactionHash, String),
}

#[derive(Debug, Clone)]
pub enum SessionKind {
    InstallUpgradeBytecode,
    GenericBytecode,
}

/// The item to be executed.
#[derive(Debug, Clone)]
pub enum ExecutableItem {
    /// Deploy model byte code.
    Deploy(Bytes),
    /// Payment byte code.
    PaymentBytes(Bytes),
    /// Session byte code.
    SessionBytes {
        /// The kind of session.
        kind: SessionKind,
        /// The compiled Wasm.
        module_bytes: Bytes,
    },
    /// An attempt to invoke a stored entity or package.
    Invocation(TransactionInvocationTarget),
}

impl ExecutableItem {
    /// Is install upgrade allowed?
    pub fn is_install_upgrade_allowed(&self) -> bool {
        match self {
            ExecutableItem::Deploy(_) => true,
            ExecutableItem::PaymentBytes(_) | ExecutableItem::Invocation(_) => false,
            ExecutableItem::SessionBytes { kind, .. } => {
                matches!(kind, SessionKind::InstallUpgradeBytecode)
            }
        }
    }
}

/// Block info.
#[derive(Copy, Clone, Debug)]
pub struct BlockInfo {
    /// State root hash of the global state in which the transaction will be executed.
    pub state_hash: Digest,
    /// Block time represented as a unix timestamp.
    pub block_time: BlockTime,
    /// Parent block hash
    pub parent_block_hash: BlockHash,
    /// Block height
    pub block_height: u64,
    /// Protocol version
    pub protocol_version: ProtocolVersion,
}

impl BlockInfo {
    /// A new instance of `[BlockInfo]`.
    pub fn new(
        state_hash: Digest,
        block_time: BlockTime,
        parent_block_hash: BlockHash,
        block_height: u64,
        protocol_version: ProtocolVersion,
    ) -> Self {
        BlockInfo {
            state_hash,
            block_time,
            parent_block_hash,
            block_height,
            protocol_version,
        }
    }

    /// Apply different state hash.
    pub fn with_state_hash(&mut self, state_hash: Digest) {
        self.state_hash = state_hash;
    }

    /// State hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Block time.
    pub fn block_time(&self) -> BlockTime {
        self.block_time
    }

    /// Parent block hash.
    pub fn parent_block_hash(&self) -> BlockHash {
        self.parent_block_hash
    }

    /// Block height.
    pub fn block_height(&self) -> u64 {
        self.block_height
    }

    /// Protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}

/// A request to execute the given Wasm on the V1 runtime.
#[derive(Debug)]
pub struct WasmV1Request {
    /// Block info.
    pub block_info: BlockInfo,
    /// The hash identifying the transaction.
    pub transaction_hash: TransactionHash,
    /// The number of Motes per unit of Gas to be paid for execution.
    pub gas_limit: Gas,
    /// The transaction's initiator.
    pub initiator_addr: InitiatorAddr,
    /// The executable item.
    pub executable_item: ExecutableItem,
    /// The entry point to call when executing.
    pub entry_point: String,
    /// The runtime args.
    pub args: RuntimeArgs,
    /// The account hashes of the signers of the transaction.
    pub authorization_keys: BTreeSet<AccountHash>,
    /// Execution phase.
    pub phase: Phase,
}

impl WasmV1Request {
    /// New from executable deploy item or InvalidRequest error.
    pub fn new_from_executable_deploy_item(
        block_info: BlockInfo,
        gas_limit: Gas,
        transaction_hash: TransactionHash,
        initiator_addr: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        session_item: &ExecutableDeployItem,
    ) -> Result<Self, InvalidRequest> {
        let executable_info =
            build_session_info_for_executable_item(session_item, transaction_hash)?;
        Ok(Self::new_from_executable_info(
            block_info,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            executable_info,
        ))
    }

    /// New payment from executable deploy item or InvalidRequest error.
    pub fn new_payment_from_executable_deploy_item(
        block_info: BlockInfo,
        gas_limit: Gas,
        transaction_hash: TransactionHash,
        initiator_addr: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        payment_item: &ExecutableDeployItem,
    ) -> Result<Self, InvalidRequest> {
        let executable_info =
            build_payment_info_for_executable_item(payment_item, transaction_hash)?;
        Ok(Self::new_from_executable_info(
            block_info,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            executable_info,
        ))
    }

    pub(crate) fn new_from_executable_info(
        block_info: BlockInfo,
        gas_limit: Gas,
        transaction_hash: TransactionHash,
        initiator_addr: InitiatorAddr,
        authorization_keys: BTreeSet<AccountHash>,
        executable_info: impl Executable,
    ) -> Self {
        let executable_item = executable_info.item();
        Self {
            block_info,
            transaction_hash,
            gas_limit,
            initiator_addr,
            authorization_keys,
            executable_item,
            entry_point: executable_info.entry_point().clone(),
            args: executable_info.args().clone(),
            phase: executable_info.phase(),
        }
    }

    /// Creates a new request from a transaction for use as the session code.
    pub fn new_session(
        block_info: BlockInfo,
        gas_limit: Gas,
        session_input_data: &SessionInputData,
    ) -> Result<Self, InvalidRequest> {
        let session_info = SessionInfo::try_from(session_input_data)?;
        let transaction_hash = session_input_data.transaction_hash();
        let initiator_addr = session_input_data.initiator_addr().clone();
        let authorization_keys = session_input_data.signers().clone();
        Ok(WasmV1Request::new_from_executable_info(
            block_info,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            session_info,
        ))
    }

    /// Creates a new request from a transaction for use as custom payment.
    pub fn new_custom_payment(
        block_info: BlockInfo,
        gas_limit: Gas,
        session_input_data: &SessionInputData,
    ) -> Result<Self, InvalidRequest> {
        let payment_info = PaymentInfo::try_from(session_input_data)?;
        let transaction_hash = session_input_data.transaction_hash();
        let initiator_addr = session_input_data.initiator_addr().clone();
        let authorization_keys = session_input_data.signers().clone();
        Ok(WasmV1Request::new_from_executable_info(
            block_info,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            payment_info,
        ))
    }
}

/// Wasm v1 result.
#[derive(Clone, Debug)]
pub struct WasmV1Result {
    /// List of transfers that happened during execution.
    transfers: Vec<Transfer>,
    /// Gas limit.
    limit: Gas,
    /// Gas consumed.
    consumed: Gas,
    /// Execution effects.
    effects: Effects,
    /// Messages emitted during execution.
    messages: Messages,
    /// Did the wasm execute successfully?
    error: Option<EngineError>,
    /// Result captured from a ret call.
    ret: Option<CLValue>,
    /// Tracking copy cache captured during execution.
    cache: Option<TrackingCopyCache>,
}

impl WasmV1Result {
    /// Creates a new instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        limit: Gas,
        consumed: Gas,
        effects: Effects,
        transfers: Vec<Transfer>,
        messages: Messages,
        error: Option<EngineError>,
        ret: Option<CLValue>,
        cache: Option<TrackingCopyCache>,
    ) -> Self {
        WasmV1Result {
            limit,
            consumed,
            effects,
            transfers,
            messages,
            error,
            ret,
            cache,
        }
    }

    /// Error, if any.
    pub fn error(&self) -> Option<&EngineError> {
        self.error.as_ref()
    }

    /// List of transfers that happened during execution.
    pub fn transfers(&self) -> &Vec<Transfer> {
        &self.transfers
    }

    /// Gas limit.
    pub fn limit(&self) -> Gas {
        self.limit
    }

    /// Gas consumed.
    pub fn consumed(&self) -> Gas {
        self.consumed
    }

    /// Execution effects.
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    /// Tracking copy cache captured during execution.
    pub fn cache(&self) -> Option<&TrackingCopyCache> {
        self.cache.as_ref()
    }

    /// Messages emitted during execution.
    pub fn messages(&self) -> &Messages {
        &self.messages
    }

    /// Result captured from a ret call.
    pub fn ret(&self) -> Option<&CLValue> {
        self.ret.as_ref()
    }

    /// Root not found.
    pub fn root_not_found(gas_limit: Gas, state_hash: Digest) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(EngineError::RootNotFound(state_hash)),
            ret: None,
            cache: None,
        }
    }

    /// Precondition failure.
    pub fn precondition_failure(gas_limit: Gas, error: EngineError) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(error),
            ret: None,
            cache: None,
        }
    }

    /// Failed to transform transaction into an executable item.
    pub fn invalid_executable_item(gas_limit: Gas, error: InvalidRequest) -> Self {
        WasmV1Result {
            transfers: Vec::default(),
            effects: Effects::new(),
            messages: Vec::default(),
            limit: gas_limit,
            consumed: Gas::zero(),
            error: Some(EngineError::InvalidExecutableItem(error)),
            ret: None,
            cache: None,
        }
    }

    /// Returns `true` if this is a precondition failure.
    ///
    /// Precondition variant is further described as an execution failure which does not have any
    /// effects, and has a gas cost of 0.
    pub fn has_precondition_failure(&self) -> bool {
        self.error.is_some() && self.consumed == Gas::zero() && self.effects.is_empty()
    }

    /// Converts a transfer result to an execution result.
    pub fn from_transfer_result(transfer_result: TransferResult, consumed: Gas) -> Option<Self> {
        // NOTE: for native / wasmless operations limit and consumed are always equal, and
        // we can get away with simplifying to one or the other here.
        // this is NOT true of wasm based operations however.
        match transfer_result {
            TransferResult::RootNotFound => None,
            TransferResult::Success {
                transfers,
                effects,
                cache,
            } => Some(WasmV1Result {
                transfers,
                limit: consumed,
                consumed,
                effects,
                messages: Messages::default(),
                error: None,
                ret: None,
                cache: Some(cache),
            }),
            TransferResult::Failure(te) => {
                Some(WasmV1Result {
                    transfers: vec![],
                    limit: consumed,
                    consumed,
                    effects: Effects::default(), // currently not returning effects on failure
                    messages: Messages::default(),
                    error: Some(EngineError::Transfer(te)),
                    ret: None,
                    cache: None,
                })
            }
        }
    }

    /// Checks effects for an AddUInt512 transform to a balance at imputed addr
    /// and for exactly the imputed amount.
    pub fn balance_increased_by_amount(&self, addr: URefAddr, amount: U512) -> bool {
        if self.effects.is_empty() || self.effects.transforms().is_empty() {
            return false;
        }

        let key = Key::Balance(addr);
        if let Some(transform) = self.effects.transforms().iter().find(|x| x.key() == &key) {
            if let TransformKindV2::AddUInt512(added) = transform.kind() {
                return *added == amount;
            }
        }
        false
    }
}

/// Helper struct to carry item, entry_point, and arg info for a `WasmV1Request`.
struct ExecutableInfo {
    item: ExecutableItem,
    entry_point: String,
    args: RuntimeArgs,
}

pub(crate) trait Executable {
    fn item(&self) -> ExecutableItem;
    fn entry_point(&self) -> &String;
    fn args(&self) -> &RuntimeArgs;
    fn phase(&self) -> Phase;
}

/// New type for hanging session specific impl's off of.
struct SessionInfo(ExecutableInfo);

impl Executable for SessionInfo {
    fn item(&self) -> ExecutableItem {
        self.0.item.clone()
    }

    fn entry_point(&self) -> &String {
        &self.0.entry_point
    }

    fn args(&self) -> &RuntimeArgs {
        &self.0.args
    }

    fn phase(&self) -> Phase {
        Phase::Session
    }
}

impl TryFrom<&SessionInputData<'_>> for PaymentInfo {
    type Error = InvalidRequest;

    fn try_from(input_data: &SessionInputData) -> Result<Self, Self::Error> {
        match input_data {
            SessionInputData::DeploySessionData { data } => PaymentInfo::try_from(data),
            SessionInputData::SessionDataV1 { data } => PaymentInfo::try_from(data),
        }
    }
}

impl TryFrom<&SessionInputData<'_>> for SessionInfo {
    type Error = InvalidRequest;

    fn try_from(input_data: &SessionInputData) -> Result<Self, Self::Error> {
        match input_data {
            SessionInputData::DeploySessionData { data } => SessionInfo::try_from(data),
            SessionInputData::SessionDataV1 { data } => SessionInfo::try_from(data),
        }
    }
}

impl TryFrom<&SessionDataDeploy<'_>> for SessionInfo {
    type Error = InvalidRequest;

    fn try_from(deploy_data: &SessionDataDeploy) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::Deploy(*deploy_data.deploy_hash());
        let session_item = deploy_data.session();
        build_session_info_for_executable_item(session_item, transaction_hash)
    }
}

fn build_session_info_for_executable_item(
    session_item: &ExecutableDeployItem,
    transaction_hash: TransactionHash,
) -> Result<SessionInfo, InvalidRequest> {
    let session: ExecutableItem;
    let session_entry_point: String;
    let session_args: RuntimeArgs;
    match session_item {
        ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
            session = ExecutableItem::Deploy(module_bytes.clone());
            session_entry_point = DEFAULT_ENTRY_POINT.to_string();
            session_args = args.clone();
        }
        ExecutableDeployItem::StoredContractByHash {
            hash,
            entry_point,
            args,
        } => {
            session = ExecutableItem::Invocation(
                TransactionInvocationTarget::new_invocable_entity((*hash).into()),
            );
            session_entry_point = entry_point.clone();
            session_args = args.clone();
        }
        ExecutableDeployItem::StoredContractByName {
            name,
            entry_point,
            args,
        } => {
            session = ExecutableItem::Invocation(
                TransactionInvocationTarget::new_invocable_entity_alias(name.clone()),
            );
            session_entry_point = entry_point.clone();
            session_args = args.clone();
        }
        ExecutableDeployItem::StoredVersionedContractByHash {
            hash,
            version,
            entry_point,
            args,
        } => {
            session = ExecutableItem::Invocation(TransactionInvocationTarget::new_package(
                PackageHash::new(hash.value()),
                *version,
            ));
            session_entry_point = entry_point.clone();
            session_args = args.clone();
        }
        ExecutableDeployItem::StoredVersionedContractByName {
            name,
            version,
            entry_point,
            args,
        } => {
            session = ExecutableItem::Invocation(TransactionInvocationTarget::new_package_alias(
                name.clone(),
                *version,
            ));
            session_entry_point = entry_point.clone();
            session_args = args.clone();
        }
        ExecutableDeployItem::Transfer { .. } => {
            return Err(InvalidRequest::UnsupportedMode(
                transaction_hash,
                session_item.to_string(),
            ));
        }
    }

    Ok(SessionInfo(ExecutableInfo {
        item: session,
        entry_point: session_entry_point,
        args: session_args,
    }))
}

impl TryFrom<&SessionDataV1<'_>> for SessionInfo {
    type Error = InvalidRequest;

    fn try_from(v1_txn: &SessionDataV1) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::V1(*v1_txn.hash());
        let args = v1_txn.args().clone();
        let session = match v1_txn.target() {
            TransactionTarget::Native => {
                return Err(InvalidRequest::InvalidTarget(
                    transaction_hash,
                    v1_txn.target().to_string(),
                ));
            }
            TransactionTarget::Stored { id, .. } => {
                let TransactionEntryPoint::Custom(entry_point) = v1_txn.entry_point() else {
                    return Err(InvalidRequest::InvalidEntryPoint(
                        transaction_hash,
                        v1_txn.entry_point().to_string(),
                    ));
                };
                let item = ExecutableItem::Invocation(id.clone());
                ExecutableInfo {
                    item,
                    entry_point: entry_point.clone(),
                    args,
                }
            }
            TransactionTarget::Session { module_bytes, .. } => {
                if *v1_txn.entry_point() != TransactionEntryPoint::Call {
                    return Err(InvalidRequest::InvalidEntryPoint(
                        transaction_hash,
                        v1_txn.entry_point().to_string(),
                    ));
                };
                let kind = if v1_txn.is_install_upgrade() {
                    SessionKind::InstallUpgradeBytecode
                } else {
                    SessionKind::GenericBytecode
                };
                let item = ExecutableItem::SessionBytes {
                    kind,
                    module_bytes: module_bytes.clone(),
                };
                ExecutableInfo {
                    item,
                    entry_point: DEFAULT_ENTRY_POINT.to_owned(),
                    args,
                }
            }
        };

        Ok(SessionInfo(session))
    }
}
/// New type for hanging payment specific impl's off of.
struct PaymentInfo(ExecutableInfo);

impl Executable for PaymentInfo {
    fn item(&self) -> ExecutableItem {
        self.0.item.clone()
    }

    fn entry_point(&self) -> &String {
        &self.0.entry_point
    }

    fn args(&self) -> &RuntimeArgs {
        &self.0.args
    }

    fn phase(&self) -> Phase {
        Phase::Payment
    }
}

impl TryFrom<&SessionDataDeploy<'_>> for PaymentInfo {
    type Error = InvalidRequest;

    fn try_from(deploy_data: &SessionDataDeploy) -> Result<Self, Self::Error> {
        let payment_item = deploy_data.session();
        let transaction_hash = TransactionHash::Deploy(*deploy_data.deploy_hash());
        build_payment_info_for_executable_item(payment_item, transaction_hash)
    }
}

fn build_payment_info_for_executable_item(
    payment_item: &ExecutableDeployItem,
    transaction_hash: TransactionHash,
) -> Result<PaymentInfo, InvalidRequest> {
    match payment_item {
        ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
            let payment = if module_bytes.is_empty() {
                return Err(InvalidRequest::UnsupportedMode(
                    transaction_hash,
                    "standard payment is no longer handled by the execution engine".to_string(),
                ));
            } else {
                ExecutableItem::PaymentBytes(module_bytes.clone())
            };
            Ok(PaymentInfo(ExecutableInfo {
                item: payment,
                entry_point: DEFAULT_ENTRY_POINT.to_string(),
                args: args.clone(),
            }))
        }
        ExecutableDeployItem::StoredContractByHash {
            hash,
            args,
            entry_point,
        } => Ok(PaymentInfo(ExecutableInfo {
            item: ExecutableItem::Invocation(TransactionInvocationTarget::ByHash(hash.value())),
            entry_point: entry_point.clone(),
            args: args.clone(),
        })),
        ExecutableDeployItem::StoredContractByName {
            name,
            args,
            entry_point,
        } => Ok(PaymentInfo(ExecutableInfo {
            item: ExecutableItem::Invocation(TransactionInvocationTarget::ByName(name.clone())),
            entry_point: entry_point.clone(),
            args: args.clone(),
        })),
        ExecutableDeployItem::StoredVersionedContractByHash {
            args,
            hash,
            version,
            entry_point,
        } => Ok(PaymentInfo(ExecutableInfo {
            item: ExecutableItem::Invocation(TransactionInvocationTarget::ByPackageHash {
                addr: hash.value(),
                version: *version,
            }),
            entry_point: entry_point.clone(),
            args: args.clone(),
        })),
        ExecutableDeployItem::StoredVersionedContractByName {
            name,
            version,
            args,
            entry_point,
        } => Ok(PaymentInfo(ExecutableInfo {
            item: ExecutableItem::Invocation(TransactionInvocationTarget::ByPackageName {
                name: name.clone(),
                version: *version,
            }),
            entry_point: entry_point.clone(),
            args: args.clone(),
        })),
        ExecutableDeployItem::Transfer { .. } => Err(InvalidRequest::UnexpectedVariant(
            transaction_hash,
            "payment item".to_string(),
        )),
    }
}

impl TryFrom<&SessionDataV1<'_>> for PaymentInfo {
    type Error = InvalidRequest;

    fn try_from(v1_txn: &SessionDataV1) -> Result<Self, Self::Error> {
        let transaction_hash = TransactionHash::V1(*v1_txn.hash());
        match v1_txn.pricing_mode() {
            mode @ PricingMode::PaymentLimited {
                standard_payment, ..
            } => {
                if *standard_payment {
                    return Err(InvalidRequest::UnsupportedMode(
                        transaction_hash,
                        mode.to_string(),
                    ));
                }
            }
            mode @ PricingMode::Fixed { .. } | mode @ PricingMode::Prepaid { .. } => {
                return Err(InvalidRequest::UnsupportedMode(
                    transaction_hash,
                    mode.to_string(),
                ));
            }
        };

        let payment = match v1_txn.target() {
            TransactionTarget::Session { module_bytes, .. } => {
                if *v1_txn.entry_point() != TransactionEntryPoint::Call {
                    return Err(InvalidRequest::InvalidEntryPoint(
                        transaction_hash,
                        v1_txn.entry_point().to_string(),
                    ));
                };
                let item = ExecutableItem::PaymentBytes(module_bytes.clone());
                ExecutableInfo {
                    item,
                    entry_point: DEFAULT_ENTRY_POINT.to_owned(),
                    args: v1_txn.args().clone(),
                }
            }
            TransactionTarget::Native | TransactionTarget::Stored { .. } => {
                return Err(InvalidRequest::InvalidTarget(
                    transaction_hash,
                    v1_txn.target().to_string(),
                ));
            }
        };

        Ok(PaymentInfo(payment))
    }
}
