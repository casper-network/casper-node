use rand::Rng;

use casper_execution_engine::core::engine_state::execute_request::ExecuteRequest;
use casper_types::{runtime_args, ProtocolVersion, RuntimeArgs, URef, U512};

use crate::{
    internal::{DeployItemBuilder, ExecuteRequestBuilder, DEFAULT_PAYMENT},
    AccountHash, Code,
};

const ARG_AMOUNT: &str = "amount";

/// Transfer Information for validating a transfer including gas usage from source
pub struct SessionTransferInfo {
    pub(crate) source_purse: URef,
    pub(crate) maybe_target_purse: Option<URef>,
    pub(crate) transfer_amount: U512,
}

impl SessionTransferInfo {
    /// Constructs a new `SessionTransferInfo` containing information for validating a transfer
    /// when `test_context.run()` occurs.
    ///
    /// Assertion will be made that `source_purse` is debited `transfer_amount` with gas costs
    /// handled. If given, assertion will be made that `maybe_target_purse` is credited
    /// `transfer_amount`
    pub fn new(
        source_purse: URef,
        maybe_target_purse: Option<URef>,
        transfer_amount: U512,
    ) -> Self {
        SessionTransferInfo {
            source_purse,
            maybe_target_purse,
            transfer_amount,
        }
    }
}

/// A single session, i.e. a single request to execute a single deploy within the test context.
pub struct Session {
    pub(crate) inner: ExecuteRequest,
    pub(crate) expect_success: bool,
    pub(crate) check_transfer_success: Option<SessionTransferInfo>,
    pub(crate) commit: bool,
}

/// Builder for a [`Session`].
pub struct SessionBuilder {
    er_builder: ExecuteRequestBuilder,
    di_builder: DeployItemBuilder,
    expect_failure: bool,
    check_transfer_success: Option<SessionTransferInfo>,
    without_commit: bool,
}

impl SessionBuilder {
    /// Constructs a new `SessionBuilder` containing a deploy with the provided session code and
    /// session args, and with default values for the account address, payment code args, gas price,
    /// authorization keys and protocol version.
    pub fn new(session_code: Code, session_args: RuntimeArgs) -> Self {
        let di_builder = DeployItemBuilder::new()
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT });
        let di_builder = match session_code {
            Code::Path(path) => di_builder.with_session_code(path, session_args),
            Code::NamedKey(name, entry_point) => {
                di_builder.with_stored_session_named_key(&name, &entry_point, session_args)
            }
            Code::Hash(hash, entry_point) => {
                di_builder.with_stored_session_hash(hash.into(), &entry_point, session_args)
            }
        };
        let expect_failure = false;
        let check_transfer_success = None;
        let without_commit = false;
        Self {
            er_builder: Default::default(),
            di_builder,
            expect_failure,
            check_transfer_success,
            without_commit,
        }
    }

    /// Returns `self` with the provided account address set.
    pub fn with_address(mut self, address: AccountHash) -> Self {
        self.di_builder = self.di_builder.with_address(address);
        self
    }

    /// Returns `self` with the provided payment code and args set.
    pub fn with_payment_code(mut self, code: Code, args: RuntimeArgs) -> Self {
        self.di_builder = match code {
            Code::Path(path) => self.di_builder.with_payment_code(path, args),
            Code::NamedKey(name, entry_point) => {
                self.di_builder
                    .with_stored_payment_named_key(&name, &entry_point, args)
            }
            Code::Hash(hash, entry_point) => {
                self.di_builder
                    .with_stored_payment_hash(hash.into(), &entry_point, args)
            }
        };
        self
    }

    /// Returns `self` with the provided block time set.
    pub fn with_block_time(mut self, block_time: u64) -> Self {
        self.er_builder = self.er_builder.with_block_time(block_time);
        self
    }

    /// Returns `self` with the provided gas price set.
    pub fn with_gas_price(mut self, price: u64) -> Self {
        self.di_builder = self.di_builder.with_gas_price(price);
        self
    }

    /// Returns `self` with the provided authorization keys set.
    pub fn with_authorization_keys(mut self, keys: &[AccountHash]) -> Self {
        self.di_builder = self.di_builder.with_authorization_keys(keys);
        self
    }

    /// Returns `self` with the provided protocol version set.
    pub fn with_protocol_version(mut self, version: ProtocolVersion) -> Self {
        self.er_builder = self.er_builder.with_protocol_version(version);
        self
    }

    /// Will disable the expect_success call during Text_Context.run() method when expected to fail.
    pub fn without_expect_success(mut self) -> Self {
        self.expect_failure = true;
        self
    }

    /// Provide SessionTransferInfo to validate transfer including gas used from source account
    pub fn with_check_transfer_success(
        mut self,
        session_transfer_info: SessionTransferInfo,
    ) -> Self {
        self.check_transfer_success = Some(session_transfer_info);
        self
    }

    /// Do not perform commit within the ['TestContext'].['run'] method.
    pub fn without_commit(mut self) -> Self {
        self.without_commit = true;
        self
    }

    /// Builds the [`Session`].
    pub fn build(self) -> Session {
        let mut rng = rand::thread_rng();
        let execute_request = self
            .er_builder
            .push_deploy(self.di_builder.with_deploy_hash(rng.gen()).build())
            .build();
        Session {
            inner: execute_request,
            expect_success: !self.expect_failure,
            check_transfer_success: self.check_transfer_success,
            commit: !self.without_commit,
        }
    }
}
