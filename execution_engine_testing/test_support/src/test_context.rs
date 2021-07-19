use casper_execution_engine::{
    core::engine_state::{
        genesis::{GenesisAccount, GenesisConfig},
        run_genesis_request::RunGenesisRequest,
    },
    shared::motes::Motes,
};
use casper_types::{bytesrepr::ToBytes, AccessRights, Key, PublicKey, URef, U512};

use crate::{
    internal::{InMemoryWasmTestBuilder, DEFAULT_GENESIS_CONFIG, DEFAULT_GENESIS_CONFIG_HASH},
    Account, AccountHash, Error, Result, Session, URefAddr, Value,
};

/// Context in which to run a test of a Wasm smart contract.
pub struct TestContext {
    inner: InMemoryWasmTestBuilder,
}

impl TestContext {
    fn maybe_purse_balance(&self, purse_uref: Option<URef>) -> Option<Motes> {
        match purse_uref {
            None => None,
            Some(purse_uref) => {
                let purse_balance = self.get_balance(purse_uref.addr());
                Some(Motes::new(purse_balance))
            }
        }
    }

    /// Runs the supplied [`Session`] checking specified expectations of the execution and
    /// subsequent commit of transforms are met.
    ///
    /// If `session` was built without
    /// [`without_expect_success()`](crate::SessionBuilder::without_expect_success) (the default)
    /// then `run()` will panic if execution of the deploy fails.
    ///
    /// If `session` was built with
    /// [`with_check_transfer_success()`](crate::SessionBuilder::with_check_transfer_success), (not
    /// the default) then `run()` will verify transfer balances including gas used.
    ///
    /// If `session` was built without
    /// [`without_commit()`](crate::SessionBuilder::without_commit) (the default), then `run()` will
    /// commit the resulting transforms.
    pub fn run(&mut self, session: Session) -> &mut Self {
        match session.check_transfer_success {
            Some(session_transfer_info) => {
                let source_initial_balance = self
                    .maybe_purse_balance(Some(session_transfer_info.source_purse))
                    .expect("source purse balance");

                let proposer_reward_starting_balance = self.inner.get_proposer_purse_balance();

                let maybe_target_initial_balance =
                    self.maybe_purse_balance(session_transfer_info.maybe_target_purse);

                let builder = self.inner.exec(session.inner);

                if session.expect_success {
                    builder.expect_success();
                }
                if session.commit {
                    builder.commit();
                }

                let transaction_fee =
                    builder.get_proposer_purse_balance() - proposer_reward_starting_balance;

                match maybe_target_initial_balance {
                    None => (),
                    Some(target_initial_balance) => {
                        let target_ending_balance = self
                            .maybe_purse_balance(session_transfer_info.maybe_target_purse)
                            .expect("target ending balance");
                        let expected_target_ending_balance = target_initial_balance
                            + Motes::new(session_transfer_info.transfer_amount);
                        if expected_target_ending_balance != target_ending_balance {
                            panic!(
                                "target ending balance does not match; expected: {}  actual: {}",
                                expected_target_ending_balance, target_ending_balance
                            );
                        }
                    }
                }

                let expected_source_ending_balance = source_initial_balance
                    - Motes::new(session_transfer_info.transfer_amount)
                    - Motes::new(transaction_fee);

                let actual_source_ending_balance = self
                    .maybe_purse_balance(Some(session_transfer_info.source_purse))
                    .expect("source ending balance");
                if expected_source_ending_balance != actual_source_ending_balance {
                    panic!(
                        "source ending balance does not match; expected: {}  actual: {}",
                        expected_source_ending_balance, actual_source_ending_balance
                    );
                }
            }
            None => {
                let builder = self.inner.exec(session.inner);
                if session.expect_success {
                    builder.expect_success();
                }
                if session.commit {
                    builder.commit();
                }
            }
        }
        self
    }

    /// Queries for a [`Value`] stored under the given `key` and `path`.
    ///
    /// Returns an [`Error`] if not found.
    pub fn query(&self, key: AccountHash, path: &[String]) -> Result<Value> {
        self.inner
            .query(None, Key::Account(key), path)
            .map(Value::new)
            .map_err(Error::from)
    }

    /// Queries for a [`Value`] stored in a dictionary under a given 'name'
    ///
    /// Returns an [`Error`] if not found.
    pub fn query_dictionary_item(
        &self,
        key: Key,
        dictionary_name: Option<String>,
        dictionary_item_key: String,
    ) -> Result<Value> {
        let empty_path = vec![];
        let dictionary_key_bytes = dictionary_item_key
            .to_bytes()
            .map_err(|_| Error::from("Could not serialize key bytes".to_string()))?;
        let address = match key {
            Key::Account(_) | Key::Hash(_) => {
                if let Some(name) = dictionary_name {
                    let path = vec![name];
                    let seed_uref = self
                        .inner
                        .query(None, key, &path)
                        .map(Value::new)
                        .map_err(Error::from)?
                        .into_t::<URef>()
                        .map_err(Error::from)?;
                    Key::dictionary(seed_uref, &dictionary_key_bytes)
                } else {
                    return Err(Error::from("No dictionary name was provided".to_string()));
                }
            }
            Key::URef(uref) => Key::dictionary(uref, &dictionary_key_bytes),
            Key::Dictionary(address) => Key::Dictionary(address),
            _ => {
                return Err(Error::from(
                    "Unsupported key type for a query to a dictionary item".to_string(),
                ))
            }
        };
        self.inner
            .query(None, address, &empty_path)
            .map(Value::new)
            .map_err(Error::from)
    }

    /// Gets the balance of the purse under the given [`URefAddr`].
    ///
    /// Note that this requires performing an earlier query to retrieve `purse_addr`.
    pub fn get_balance(&self, purse_addr: URefAddr) -> U512 {
        let purse = URef::new(purse_addr, AccessRights::READ);
        self.inner.get_purse_balance(purse)
    }

    /// Gets the main purse [`URef`] from an [`Account`] stored under a [`PublicKey`], or `None`.
    pub fn main_purse_address(&self, account_key: AccountHash) -> Option<URef> {
        self.inner
            .get_account(account_key)
            .map(|account| account.main_purse())
    }

    // TODO: Remove this once test can use query
    /// Gets an [`Account`] stored under a [`PublicKey`], or `None`.
    pub fn get_account(&self, account_key: AccountHash) -> Option<Account> {
        self.inner
            .get_account(account_key)
            .map(|account| account.into())
    }
}

/// Builder for a [`TestContext`].
pub struct TestContextBuilder {
    genesis_config: GenesisConfig,
}

impl TestContextBuilder {
    /// Constructs a new `TestContextBuilder` initialised with default values for an account, i.e.
    /// an account at [`DEFAULT_ACCOUNT_ADDR`](static@crate::DEFAULT_ACCOUNT_ADDR) with an initial
    /// balance of [`DEFAULT_ACCOUNT_INITIAL_BALANCE`](crate::DEFAULT_ACCOUNT_INITIAL_BALANCE)
    /// which will be added to the Genesis block.
    pub fn new() -> Self {
        TestContextBuilder {
            genesis_config: DEFAULT_GENESIS_CONFIG.clone(),
        }
    }

    /// Returns `self` with the provided account's details added to existing ones, for inclusion in
    /// the Genesis block.
    ///
    /// Note: `initial_balance` represents the number of motes.
    pub fn with_public_key(mut self, public_key: PublicKey, initial_balance: U512) -> Self {
        let new_account = GenesisAccount::account(public_key, Motes::new(initial_balance), None);
        self.genesis_config
            .ee_config_mut()
            .push_account(new_account);
        self
    }

    /// Builds the [`TestContext`].
    pub fn build(self) -> TestContext {
        let mut inner = InMemoryWasmTestBuilder::default();
        let run_genesis_request = RunGenesisRequest::new(
            *DEFAULT_GENESIS_CONFIG_HASH,
            self.genesis_config.protocol_version(),
            self.genesis_config.take_ee_config(),
        );
        inner.run_genesis(&run_genesis_request);
        TestContext { inner }
    }
}

impl Default for TestContextBuilder {
    fn default() -> Self {
        TestContextBuilder::new()
    }
}
