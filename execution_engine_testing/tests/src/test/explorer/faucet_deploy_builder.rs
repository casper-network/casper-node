use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::core::engine_state::ExecuteRequest;
use casper_types::{runtime_args, system::mint, PublicKey, U512};

use super::{
    ARG_AMOUNT, ARG_AVAILABLE_AMOUNT, ARG_DISTRIBUTIONS_PER_INTERVAL, ARG_ID, ARG_TARGET,
    ARG_TIME_INTERVAL, ENTRY_POINT_AUTHORIZE_TO, ENTRY_POINT_SET_VARIABLES,
    FAUCET_CONTRACT_NAMED_KEY, FAUCET_FUND_AMOUNT, FAUCET_ID, FAUCET_INSTALLER_SESSION,
    INSTALLER_ACCOUNT, INSTALLER_FUND_AMOUNT,
};

pub struct FundInstallerRequestBuilder {
    installer_account: AccountHash,
    installer_fund_amount: U512,
    installer_fund_id: Option<u64>,
}

impl FundInstallerRequestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_installer_account(&mut self, account_hash: AccountHash) -> Self {
        self.installer_account = account_hash;
        self
    }

    pub fn with_installer_fund_amount(&mut self, fund_amount: U512) -> Self {
        self.installer_fund_amount = fund_amount;
        self
    }

    pub fn with_installer_fund_id(&mut self, fund_id: Option<u64>) -> Self {
        self.installer_fund_id = fund_id;
        self
    }

    pub fn build(self) -> ExecuteRequest {
        ExecuteRequestBuilder::transfer(
            *DEFAULT_ACCOUNT_ADDR,
            runtime_args! {
                mint::ARG_TARGET => self.installer_account,
                mint::ARG_AMOUNT => self.installer_fund_amount,
                mint::ARG_ID => self.fund_id
            },
        )
        .build()
    }
}

impl Default for FundInstallerRequestBuilder {
    fn default() -> Self {
        Self {
            installer_account: *INSTALLER_ACCOUNT,
            installer_fund_amount: INSTALLER_FUND_AMOUNT,
            installer_fund_id: None,
        }
    }
}

pub struct FaucetInstallSessionRequestBuilder {
    installer_account: AccountHash,
    faucet_installer_session: String,
    faucet_id: u64,
    faucet_fund_amount: U512,
}

impl FaucetInstallSessionRequestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_installer_account(&mut self, installer_account: AccountHash) -> Self {
        self.installer_account = installer_account;
        self
    }

    pub fn with_faucet_installer_session(&mut self, installer_session: &str) -> Self {
        self.faucet_installer_session = installer_session.to_string();
        self
    }

    pub fn with_faucet_id(&mut self, faucet_id: u64) -> Self {
        self.faucet_id = faucet_id;
        self
    }

    pub fn with_faucet_fund_amount<T>(&mut self, faucet_fund_amount: U512) -> Self {
        self.faucet_fund_amount = faucet_fund_amount;
        self
    }

    pub fn build(self) -> ExecuteRequest {
        ExecuteRequestBuilder::standard(
            self.installer_account,
            &self.faucet_installer_session,
            runtime_args! {
                ARG_ID => self.faucet_id,
                ARG_AMOUNT => self.faucet_fund_amount
            },
        )
        .build()
    }
}

impl Default for FaucetInstallSessionRequestBuilder {
    fn default() -> Self {
        Self {
            installer_account: *INSTALLER_ACCOUNT,
            faucet_installer_session: FAUCET_INSTALLER_SESSION,
            faucet_id: FAUCET_ID,
            faucet_fund_amount: FAUCET_FUND_AMOUNT.into(),
        }
    }
}

struct FaucetConfigRequestBuilder {
    installer_account: AccountHash,
    faucet_contract_hash: Option<ContractHash>,
    available_amount: Option<U512>,
    time_interval: Option<u64>,
    distributions_per_interval: Option<u64>,
}

impl FaucetConfigRequestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_installer_account(&mut self, installer_account: AccountHash) -> Self {
        self.installer_account = installer_account;
        self
    }

    pub fn with_faucet_contract_hash(&mut self, contract_hash: ContractHash) -> Self {
        self.faucet_contract_hash = Some(contract_hash);
        self
    }

    pub fn with_available_amount(&mut self, available_amount: Option<U512>) -> Self {
        self.available_amount = available_amount;
        self
    }

    pub fn with_time_interval(&mut self, time_interval: Option<u64>) -> Self {
        self.time_interval = time_interval;
        self
    }

    pub fn with_distributions_per_interval(
        &mut self,
        distributions_per_interval: Option<u64>,
    ) -> Self {
        self.distributions_per_interval = distributions_per_interval;
        self
    }

    pub fn build(self) -> ExecuteRequest {
        ExecuteRequestBuilder::contract_call_by_hash(
            self.installer_account,
            self.faucet_contract_hash,
            ENTRY_POINT_SET_VARIABLES,
            runtime_args! {
                ARG_AVAILABLE_AMOUNT => self.available_amount,
                ARG_TIME_INTERVAL => self.time_interval,
                ARG_DISTRIBUTIONS_PER_INTERVAL => self.distributions_per_interval
            },
        )
        .build()
    }
}

impl Default for FaucetConfigRequestBuilder {
    fn default() -> Self {
        Self {
            installer_account: *INSTALLER_ACCOUNT,
            faucet_contract_hash: None,
            available_amount: None,
            time_interval: None,
            distributions_per_interval: None,
        }
    }
}

pub struct FaucetAuthorizeAccountRequestBuilder {
    installer_account: AccountHash,
    authorized_account_public_key: Option<PublicKey>,
    faucet_contract_hash: Option<ContractHash>,
}

impl FaucetAuthorizeUserRequestBuilder {
    pub fn with_installer_account(&mut self, installer_account: AccountHash) -> Self {
        self.installer_account = installer_account;
        self
    }

    pub fn with_authorized_user_public_key(
        &mut self,
        authorized_account_public_key: Option<PublicKey>,
    ) -> Self {
        self.authorized_account_public_key = authorized_account_public_key
    }

    pub fn build(self) -> ExecuteRequest {
        ExecuteRequestBuilder::contract_call_by_hash(
            installer_account,
            self.faucet_contract_hash,
            ENTRY_POINT_AUTHORIZE_TO,
            runtime_args! {ARG_TARGET => self.authorized_account_public_key},
        )
        .build()
    }
}

impl Default for FaucetAuthorizeAccountRequestBuilder {
    fn default() -> Self {
        Self {
            installer_account: *INSTALLER_ACCOUNT,
            authorized_account_public_key: None,
            faucet_contract_hash: None,
        }
    }
}

pub struct FaucetFundRequestBuilder {
    installer_account: AccountHash,
    authorized_account: Option<AccountHash>,
    user_account: Option<AccountHash>,
}

impl FaucetFundRequestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_installer_account(&mut self, installer_account: AccountHash) -> Self {
        self.installer_account = installer_account;
        self
    }

    pub fn with_authorized_account(&mut self, authorized_account: Option<AccountHash>) -> Self {
        self.authorized_account = authorized_account;
        self
    }

    pub fn with_user_account(&mut self, user_account: Option<AccountHash>) -> Self {
        self.user_account = user_account;
        self
    }
}

impl Default for FaucetFundRequestBuilder {
    fn default() -> Self {
        Self {
            installer_account: *INSTALLER_ACCOUNT,
            authorized_account: None,
            user_account: None,
        }
    }
}
