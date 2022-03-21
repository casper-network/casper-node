use rand::Rng;

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::core::engine_state::ExecuteRequest;
use casper_types::{
    account::AccountHash, bytesrepr::FromBytes, runtime_args, system::mint, CLTyped, ContractHash,
    Key, PublicKey, RuntimeArgs, U512,
};

use super::{
    ARG_AMOUNT, ARG_AVAILABLE_AMOUNT, ARG_DISTRIBUTIONS_PER_INTERVAL, ARG_ID, ARG_TARGET,
    ARG_TIME_INTERVAL, ENTRY_POINT_AUTHORIZE_TO, ENTRY_POINT_FAUCET, ENTRY_POINT_SET_VARIABLES,
    FAUCET_CONTRACT_NAMED_KEY, FAUCET_FUND_AMOUNT, FAUCET_ID, FAUCET_INSTALLER_SESSION,
    INSTALLER_ACCOUNT, INSTALLER_FUND_AMOUNT,
};

pub struct FundAccountRequestBuilder {
    target_account: AccountHash,
    fund_amount: U512,
    fund_id: Option<u64>,
}

impl FundAccountRequestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_target_account(mut self, account_hash: AccountHash) -> Self {
        self.target_account = account_hash;
        self
    }

    pub fn with_fund_amount(mut self, fund_amount: U512) -> Self {
        self.fund_amount = fund_amount;
        self
    }

    pub fn with_fund_id(mut self, fund_id: Option<u64>) -> Self {
        self.fund_id = fund_id;
        self
    }

    pub fn build(self) -> ExecuteRequest {
        ExecuteRequestBuilder::transfer(
            *DEFAULT_ACCOUNT_ADDR,
            runtime_args! {
                mint::ARG_TARGET => self.target_account,
                mint::ARG_AMOUNT => self.fund_amount,
                mint::ARG_ID => self.fund_id
            },
        )
        .build()
    }
}

impl Default for FundAccountRequestBuilder {
    fn default() -> Self {
        Self {
            target_account: *INSTALLER_ACCOUNT,
            fund_amount: U512::from(INSTALLER_FUND_AMOUNT),
            fund_id: None,
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

    pub fn with_installer_account(mut self, installer_account: AccountHash) -> Self {
        self.installer_account = installer_account;
        self
    }

    pub fn with_faucet_installer_session(mut self, installer_session: &str) -> Self {
        self.faucet_installer_session = installer_session.to_string();
        self
    }

    pub fn with_faucet_id(mut self, faucet_id: u64) -> Self {
        self.faucet_id = faucet_id;
        self
    }

    pub fn with_faucet_fund_amount(mut self, faucet_fund_amount: U512) -> Self {
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
            faucet_installer_session: FAUCET_INSTALLER_SESSION.to_string(),
            faucet_id: FAUCET_ID,
            faucet_fund_amount: FAUCET_FUND_AMOUNT.into(),
        }
    }
}

pub struct FaucetConfigRequestBuilder {
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

    pub fn with_installer_account(mut self, installer_account: AccountHash) -> Self {
        self.installer_account = installer_account;
        self
    }

    pub fn with_faucet_contract_hash(mut self, contract_hash: ContractHash) -> Self {
        self.faucet_contract_hash = Some(contract_hash);
        self
    }

    pub fn with_available_amount(mut self, available_amount: Option<U512>) -> Self {
        self.available_amount = available_amount;
        self
    }

    pub fn with_time_interval(mut self, time_interval: Option<u64>) -> Self {
        self.time_interval = time_interval;
        self
    }

    pub fn with_distributions_per_interval(
        mut self,
        distributions_per_interval: Option<u64>,
    ) -> Self {
        self.distributions_per_interval = distributions_per_interval;
        self
    }

    pub fn build(self) -> ExecuteRequest {
        ExecuteRequestBuilder::contract_call_by_hash(
            self.installer_account,
            self.faucet_contract_hash
                .expect("must supply faucet contract hash"),
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

impl FaucetAuthorizeAccountRequestBuilder {
    pub fn with_installer_account(mut self, installer_account: AccountHash) -> Self {
        self.installer_account = installer_account;
        self
    }

    pub fn with_authorized_user_public_key(
        mut self,
        authorized_account_public_key: Option<PublicKey>,
    ) -> Self {
        self.authorized_account_public_key = authorized_account_public_key;
        self
    }

    pub fn build(self) -> ExecuteRequest {
        ExecuteRequestBuilder::contract_call_by_hash(
            self.installer_account,
            self.faucet_contract_hash
                .expect("must supply faucet contract hash"),
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

enum FaucetCallerAccount {
    Installer(AccountHash),
    Authorized(AccountHash),
    User(AccountHash),
}

impl FaucetCallerAccount {
    pub fn account_hash(&self) -> AccountHash {
        match self {
            FaucetCallerAccount::Installer(account_hash)
            | FaucetCallerAccount::Authorized(account_hash)
            | FaucetCallerAccount::User(account_hash) => account_hash.clone(),
        }
    }
}

pub struct FaucetFundRequestBuilder {
    faucet_contract_hash: Option<ContractHash>,
    caller_account: FaucetCallerAccount,
    target_account: Option<AccountHash>,
    fund_amount: Option<U512>,
    fund_id: Option<u64>,
    payment_amount: U512,
}

impl FaucetFundRequestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_installer_account(mut self, installer_account: AccountHash) -> Self {
        self.caller_account = FaucetCallerAccount::Installer(installer_account);
        self
    }

    pub fn with_authorized_account(mut self, authorized_account: AccountHash) -> Self {
        self.caller_account = FaucetCallerAccount::Authorized(authorized_account);
        self
    }

    pub fn with_user_account(mut self, user_account: AccountHash) -> Self {
        self.caller_account = FaucetCallerAccount::User(user_account);
        self
    }

    pub fn with_fund_amount(mut self, fund_amount: U512) -> Self {
        self.fund_amount = Some(fund_amount);
        self
    }

    pub fn with_payment_amount(mut self, payment_amount: U512) -> Self {
        self.payment_amount = payment_amount;
        self
    }

    pub fn build(self) -> ExecuteRequest {
        let mut rng = rand::thread_rng();

        let deploy_item = DeployItemBuilder::new()
            .with_address(self.caller_account.account_hash())
            .with_authorization_keys(&[self.caller_account.account_hash()])
            .with_stored_session_hash(
                self.faucet_contract_hash
                    .expect("must supply faucet contract hash"),
                ENTRY_POINT_FAUCET,
                runtime_args! {
                    ARG_TARGET => self.target_account.unwrap(),
                    ARG_AMOUNT => self.fund_amount,
                    ARG_ID => self.fund_id
                },
            )
            .with_empty_payment_bytes(runtime_args! {ARG_AMOUNT => self.payment_amount})
            .with_deploy_hash(rng.gen())
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item).build()
    }
}

impl Default for FaucetFundRequestBuilder {
    fn default() -> Self {
        Self {
            fund_amount: None,
            payment_amount: U512::from(3_000_000_000u64),
            faucet_contract_hash: None,
            caller_account: FaucetCallerAccount::Installer(*INSTALLER_ACCOUNT),
            target_account: None,
            fund_id: None,
        }
    }
}

pub fn query_stored_value<T: CLTyped + FromBytes>(
    builder: &mut InMemoryWasmTestBuilder,
    base_key: Key,
    path: Vec<String>,
) -> T {
    builder
        .query(None, base_key, &path)
        .expect("must have stored value")
        .as_cl_value()
        .cloned()
        .expect("must have cl value")
        .into_t::<T>()
        .expect("must get value")
}

pub fn get_faucet_contract_hash(
    builder: &InMemoryWasmTestBuilder,
    installer_account: AccountHash,
) -> ContractHash {
    builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(&format!("{}_{}", FAUCET_CONTRACT_NAMED_KEY, FAUCET_ID))
        .cloned()
        .and_then(Key::into_hash)
        .map(ContractHash::new)
        .expect("failed to find faucet contract")
}

pub fn get_faucet_key(builder: &InMemoryWasmTestBuilder, installer_account: AccountHash) -> Key {
    builder
        .get_expected_account(installer_account)
        .named_keys()
        .get(&format!("{}_{}", FAUCET_CONTRACT_NAMED_KEY, FAUCET_ID))
        .cloned()
        .expect("failed to find faucet key")
}
