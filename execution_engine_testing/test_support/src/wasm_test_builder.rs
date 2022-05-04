use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    ffi::OsStr,
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use filesize::PathExt;
use lmdb::DatabaseFlags;
use log::LevelFilter;
use num_rational::Ratio;
use num_traits::CheckedMul;

use casper_execution_engine::{
    core::{
        engine_state::{
            self,
            era_validators::GetEraValidatorsRequest,
            execute_request::ExecuteRequest,
            execution_result::ExecutionResult,
            run_genesis_request::RunGenesisRequest,
            step::{StepRequest, StepSuccess},
            BalanceResult, EngineConfig, EngineState, Error, GenesisSuccess, GetBidsRequest,
            QueryRequest, QueryResult, RewardItem, StepError, SystemContractRegistry,
            UpgradeConfig, UpgradeSuccess, DEFAULT_MAX_QUERY_DEPTH,
        },
        execution,
    },
    shared::{
        additive_map::AdditiveMap,
        execution_journal::ExecutionJournal,
        logging::{self, Settings, Style},
        newtypes::CorrelationId,
        system_config::{
            auction_costs::AuctionCosts, handle_payment_costs::HandlePaymentCosts,
            mint_costs::MintCosts,
        },
        transform::Transform,
        utils::OS_PAGE_SIZE,
    },
    storage::{
        global_state::{
            in_memory::InMemoryGlobalState, lmdb::LmdbGlobalState, scratch::ScratchGlobalState,
            CommitProvider, StateProvider, StateReader,
        },
        transaction_source::lmdb::LmdbEnvironment,
        trie::{merkle_proof::TrieMerkleProof, Trie},
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_hashing::Digest;
use casper_types::{
    account::{Account, AccountHash},
    bytesrepr::{self, FromBytes},
    runtime_args,
    system::{
        auction::{
            Bids, EraValidators, UnbondingPurses, ValidatorWeights, WithdrawPurses,
            ARG_ERA_END_TIMESTAMP_MILLIS, ARG_EVICTED_VALIDATORS, AUCTION_DELAY_KEY, ERA_ID_KEY,
            METHOD_RUN_AUCTION, UNBONDING_DELAY_KEY,
        },
        mint::{ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY},
        AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
    },
    CLTyped, CLValue, Contract, ContractHash, ContractPackage, ContractPackageHash, ContractWasm,
    DeployHash, DeployInfo, EraId, Gas, Key, KeyTag, ProtocolVersion, PublicKey, RuntimeArgs,
    StoredValue, Transfer, TransferAddr, URef, U512,
};

use crate::{
    chainspec_config::{ChainspecConfig, PRODUCTION_PATH},
    utils, ExecuteRequestBuilder, StepRequestBuilder, DEFAULT_PROPOSER_ADDR,
    DEFAULT_PROTOCOL_VERSION, SYSTEM_ADDR,
};

/// LMDB initial map size is calculated based on DEFAULT_LMDB_PAGES and systems page size.
const DEFAULT_LMDB_PAGES: usize = 256_000_000;

/// LMDB max readers
///
/// The default value is chosen to be the same as the node itself.
const DEFAULT_MAX_READERS: u32 = 512;

/// This is appended to the data dir path provided to the `LmdbWasmTestBuilder`".
const GLOBAL_STATE_DIR: &str = "global_state";

/// Wasm test builder where state is held entirely in memory.
pub type InMemoryWasmTestBuilder = WasmTestBuilder<InMemoryGlobalState>;
/// Wasm test builder where state is held in LMDB.
pub type LmdbWasmTestBuilder = WasmTestBuilder<LmdbGlobalState>;

/// Builder for simple WASM test
pub struct WasmTestBuilder<S> {
    /// [`EngineState`] is wrapped in [`Rc`] to work around a missing [`Clone`] implementation
    engine_state: Rc<EngineState<S>>,
    /// [`ExecutionResult`] is wrapped in [`Rc`] to work around a missing [`Clone`] implementation
    exec_results: Vec<Vec<Rc<ExecutionResult>>>,
    upgrade_results: Vec<Result<UpgradeSuccess, engine_state::Error>>,
    /// Genesis hash.
    genesis_hash: Option<Digest>,
    /// Post state hash.
    post_state_hash: Option<Digest>,
    /// Cached transform maps after subsequent successful runs i.e. `transforms[0]` is for first
    /// exec call etc.
    transforms: Vec<ExecutionJournal>,
    /// Cached genesis transforms
    genesis_account: Option<Account>,
    /// Genesis transforms
    genesis_transforms: Option<AdditiveMap<Key, Transform>>,
    /// Scratch global state used for in-memory execution and commit optimization.
    scratch_engine_state: Option<EngineState<ScratchGlobalState>>,
    /// System contract registry
    system_contract_registry: Option<SystemContractRegistry>,
    /// Global state dir, for implementations that define one.
    global_state_dir: Option<PathBuf>,
}

impl<S> WasmTestBuilder<S> {
    fn initialize_logging() {
        let log_settings = Settings::new(LevelFilter::Error).with_style(Style::HumanReadable);
        let _ = logging::initialize(log_settings);
    }
}

impl Default for InMemoryWasmTestBuilder {
    fn default() -> Self {
        Self::new_with_chainspec(&*PRODUCTION_PATH, None)
    }
}

// TODO: Deriving `Clone` for `WasmTestBuilder<S>` doesn't work correctly (unsure why), so
// implemented by hand here.  Try to derive in the future with a different compiler version.
impl<S> Clone for WasmTestBuilder<S> {
    fn clone(&self) -> Self {
        WasmTestBuilder {
            engine_state: Rc::clone(&self.engine_state),
            exec_results: self.exec_results.clone(),
            upgrade_results: self.upgrade_results.clone(),
            genesis_hash: self.genesis_hash,
            post_state_hash: self.post_state_hash,
            transforms: self.transforms.clone(),
            genesis_account: self.genesis_account.clone(),
            genesis_transforms: self.genesis_transforms.clone(),
            scratch_engine_state: None,
            system_contract_registry: self.system_contract_registry.clone(),
            global_state_dir: self.global_state_dir.clone(),
        }
    }
}

impl InMemoryWasmTestBuilder {
    /// Returns an [`InMemoryWasmTestBuilder`].
    pub fn new(
        global_state: InMemoryGlobalState,
        engine_config: EngineConfig,
        maybe_post_state_hash: Option<Digest>,
    ) -> Self {
        Self::initialize_logging();
        let engine_state = EngineState::new(global_state, engine_config);
        WasmTestBuilder {
            exec_results: Vec::new(),
            upgrade_results: Vec::new(),
            engine_state: Rc::new(engine_state),
            genesis_hash: maybe_post_state_hash,
            post_state_hash: maybe_post_state_hash,
            transforms: Vec::new(),
            genesis_account: None,
            genesis_transforms: None,
            scratch_engine_state: None,
            system_contract_registry: None,
            global_state_dir: None,
        }
    }

    /// Returns an [`InMemoryWasmTestBuilder`] instantiated using values from a given chainspec.
    pub fn new_with_chainspec<P: AsRef<Path>>(
        chainspec_path: P,
        post_state_hash: Option<Digest>,
    ) -> Self {
        let chainspec_config = ChainspecConfig::from_chainspec_path(chainspec_path)
            .expect("must build chainspec configuration");

        let engine_config = EngineConfig::new(
            DEFAULT_MAX_QUERY_DEPTH,
            chainspec_config.core_config.max_associated_keys,
            chainspec_config.core_config.max_runtime_call_stack_height,
            chainspec_config.core_config.minimum_delegation_amount,
            chainspec_config.core_config.strict_argument_checking,
            chainspec_config.wasm_config,
            chainspec_config.system_costs_config,
        );

        let global_state = InMemoryGlobalState::empty().expect("should create global state");

        Self::new(global_state, engine_config, post_state_hash)
    }
}

impl LmdbWasmTestBuilder {
    /// Returns an [`LmdbWasmTestBuilder`] with configuration.
    pub fn new_with_config<T: AsRef<OsStr> + ?Sized>(
        data_dir: &T,
        engine_config: EngineConfig,
    ) -> Self {
        Self::initialize_logging();
        let page_size = *OS_PAGE_SIZE;
        let global_state_dir = Self::global_state_dir(data_dir);
        Self::create_global_state_dir(&global_state_dir);
        let environment = Arc::new(
            LmdbEnvironment::new(
                &global_state_dir,
                page_size * DEFAULT_LMDB_PAGES,
                DEFAULT_MAX_READERS,
                true,
            )
            .expect("should create LmdbEnvironment"),
        );
        let trie_store = Arc::new(
            LmdbTrieStore::new(&environment, None, DatabaseFlags::empty())
                .expect("should create LmdbTrieStore"),
        );

        let global_state =
            LmdbGlobalState::empty(environment, trie_store).expect("should create LmdbGlobalState");
        let engine_state = EngineState::new(global_state, engine_config);
        WasmTestBuilder {
            engine_state: Rc::new(engine_state),
            exec_results: Vec::new(),
            upgrade_results: Vec::new(),
            genesis_hash: None,
            post_state_hash: None,
            transforms: Vec::new(),
            genesis_account: None,
            genesis_transforms: None,
            scratch_engine_state: None,
            system_contract_registry: None,
            global_state_dir: Some(global_state_dir),
        }
    }

    /// Returns an [`LmdbWasmTestBuilder`] with configuration and values from
    /// a given chainspec.
    pub fn new_with_chainspec<T: AsRef<OsStr> + ?Sized, P: AsRef<Path>>(
        data_dir: &T,
        chainspec_path: P,
    ) -> Self {
        let chainspec_config = ChainspecConfig::from_chainspec_path(chainspec_path)
            .expect("must build chainspec configuration");

        let engine_config = EngineConfig::new(
            DEFAULT_MAX_QUERY_DEPTH,
            chainspec_config.core_config.max_associated_keys,
            chainspec_config.core_config.max_runtime_call_stack_height,
            chainspec_config.core_config.minimum_delegation_amount,
            chainspec_config.core_config.strict_argument_checking,
            chainspec_config.wasm_config,
            chainspec_config.system_costs_config,
        );

        Self::new_with_config(data_dir, engine_config)
    }

    /// Returns an [`LmdbWasmTestBuilder`] with configuration and values from
    /// the production chainspec.
    pub fn new_with_production_chainspec<T: AsRef<OsStr> + ?Sized>(data_dir: &T) -> Self {
        Self::new_with_chainspec(data_dir, &*PRODUCTION_PATH)
    }

    /// Flushes the LMDB environment to disk.
    pub fn flush_environment(&self) {
        let engine_state = &*self.engine_state;
        engine_state.flush_environment().unwrap();
    }

    /// Returns a new [`LmdbWasmTestBuilder`].
    pub fn new<T: AsRef<OsStr> + ?Sized>(data_dir: &T) -> Self {
        Self::new_with_config(data_dir, Default::default())
    }

    /// Creates a new instance of builder using the supplied configurations, opening wrapped LMDBs
    /// (e.g. in the Trie and Data stores) rather than creating them.
    pub fn open<T: AsRef<OsStr> + ?Sized>(
        data_dir: &T,
        engine_config: EngineConfig,
        post_state_hash: Digest,
    ) -> Self {
        let global_state_path = Self::global_state_dir(data_dir);
        Self::open_raw(global_state_path, engine_config, post_state_hash)
    }

    /// Creates a new instance of builder using the supplied configurations, opening wrapped LMDBs
    /// (e.g. in the Trie and Data stores) rather than creating them.
    /// Differs from `open` in that it doesn't append `GLOBAL_STATE_DIR` to the supplied path.
    pub fn open_raw<T: AsRef<Path>>(
        global_state_dir: T,
        engine_config: EngineConfig,
        post_state_hash: Digest,
    ) -> Self {
        Self::initialize_logging();
        let page_size = *OS_PAGE_SIZE;
        Self::create_global_state_dir(&global_state_dir);
        let environment = Arc::new(
            LmdbEnvironment::new(
                &global_state_dir,
                page_size * DEFAULT_LMDB_PAGES,
                DEFAULT_MAX_READERS,
                true,
            )
            .expect("should create LmdbEnvironment"),
        );
        let trie_store =
            Arc::new(LmdbTrieStore::open(&environment, None).expect("should open LmdbTrieStore"));

        let global_state =
            LmdbGlobalState::empty(environment, trie_store).expect("should create LmdbGlobalState");
        let engine_state = EngineState::new(global_state, engine_config);
        WasmTestBuilder {
            engine_state: Rc::new(engine_state),
            exec_results: Vec::new(),
            upgrade_results: Vec::new(),
            genesis_hash: None,
            post_state_hash: Some(post_state_hash),
            transforms: Vec::new(),
            genesis_account: None,
            genesis_transforms: None,
            scratch_engine_state: None,
            system_contract_registry: None,
            global_state_dir: Some(global_state_dir.as_ref().to_path_buf()),
        }
    }

    fn create_global_state_dir<T: AsRef<Path>>(global_state_path: T) {
        fs::create_dir_all(&global_state_path).unwrap_or_else(|_| {
            panic!(
                "Expected to create {}",
                global_state_path.as_ref().display()
            )
        });
    }

    fn global_state_dir<T: AsRef<OsStr> + ?Sized>(data_dir: &T) -> PathBuf {
        let mut path = PathBuf::from(data_dir);
        path.push(GLOBAL_STATE_DIR);
        path
    }

    /// Returns the file size on disk of the backing lmdb file behind DbGlobalState.
    pub fn lmdb_on_disk_size(&self) -> Option<u64> {
        if let Some(path) = self.global_state_dir.as_ref() {
            let mut path = path.clone();
            path.push("data.lmdb");
            return path.as_path().size_on_disk().ok();
        }
        None
    }

    /// Execute and commit transforms from an ExecuteRequest into a scratch global state.
    /// You MUST call write_scratch_to_lmdb to flush these changes to LmdbGlobalState.
    pub fn scratch_exec_and_commit(&mut self, mut exec_request: ExecuteRequest) -> &mut Self {
        if self.scratch_engine_state.is_none() {
            self.scratch_engine_state = Some(self.engine_state.get_scratch_engine_state());
        }

        let cached_state = self
            .scratch_engine_state
            .as_ref()
            .expect("scratch state should exist");

        // Scratch still requires that one deploy be executed and committed at a time.
        let exec_request = {
            let hash = self.post_state_hash.expect("expected post_state_hash");
            exec_request.parent_state_hash = hash;
            exec_request
        };

        let mut exec_results = Vec::new();
        // First execute the request against our scratch global state.
        let maybe_exec_results = cached_state.run_execute(CorrelationId::new(), exec_request);
        for execution_result in maybe_exec_results.unwrap() {
            let journal = execution_result.execution_journal().clone();
            let transforms: AdditiveMap<Key, Transform> = journal.clone().into();
            let _post_state_hash = cached_state
                .apply_effect(
                    CorrelationId::new(),
                    self.post_state_hash.expect("requires a post_state_hash"),
                    transforms,
                )
                .expect("should commit");

            // Save transforms and execution results for WasmTestBuilder.
            self.transforms.push(journal);
            exec_results.push(Rc::new(execution_result))
        }
        self.exec_results.push(exec_results);
        self
    }

    /// Commit scratch to global state, and reset the scratch cache.
    pub fn write_scratch_to_lmdb(&mut self) -> &mut Self {
        let prestate_hash = self.post_state_hash.expect("Should have genesis hash");
        if let Some(scratch) = self.scratch_engine_state.take() {
            let new_state_root = self
                .engine_state
                .write_scratch_to_lmdb(prestate_hash, scratch.into_inner())
                .unwrap();
            self.post_state_hash = Some(new_state_root);
        }
        self
    }

    /// run step against scratch global state.
    pub fn step_with_scratch(&mut self, step_request: StepRequest) -> &mut Self {
        if self.scratch_engine_state.is_none() {
            self.scratch_engine_state = Some(self.engine_state.get_scratch_engine_state());
        }

        let cached_state = self
            .scratch_engine_state
            .as_ref()
            .expect("scratch state should exist");

        cached_state
            .commit_step(CorrelationId::new(), step_request)
            .expect("unable to run step request against scratch global state");
        self
    }
}

impl<S> WasmTestBuilder<S>
where
    S: StateProvider + CommitProvider,
    engine_state::Error: From<S::Error>,
    S::Error: Into<execution::Error>,
{
    /// Takes a [`RunGenesisRequest`], executes the request and returns Self.
    pub fn run_genesis(&mut self, run_genesis_request: &RunGenesisRequest) -> &mut Self {
        let system_account = Key::Account(PublicKey::System.to_account_hash());

        let GenesisSuccess {
            post_state_hash,
            execution_effect,
        } = self
            .engine_state
            .commit_genesis(
                CorrelationId::new(),
                run_genesis_request.genesis_config_hash(),
                run_genesis_request.protocol_version(),
                run_genesis_request.ee_config(),
                run_genesis_request.chainspec_registry().clone(),
            )
            .expect("Unable to get genesis response");

        let transforms = execution_effect.transforms;
        let empty_path: Vec<String> = vec![];

        let genesis_account =
            utils::get_account(&transforms, &system_account).expect("Unable to get system account");

        self.system_contract_registry = match self.query(
            Some(post_state_hash),
            Key::SystemContractRegistry,
            &empty_path,
        ) {
            Ok(StoredValue::CLValue(cl_registry)) => {
                let system_contract_registry =
                    CLValue::into_t::<SystemContractRegistry>(cl_registry).unwrap();
                Some(system_contract_registry)
            }
            Ok(_) => panic!("Failed to get system registry"),
            Err(err) => panic!("{}", err),
        };

        self.genesis_hash = Some(post_state_hash);
        self.post_state_hash = Some(post_state_hash);
        self.genesis_account = Some(genesis_account);
        self.genesis_transforms = Some(transforms);
        self
    }

    /// Queries state for a [`StoredValue`].
    pub fn query(
        &self,
        maybe_post_state: Option<Digest>,
        base_key: Key,
        path: &[String],
    ) -> Result<StoredValue, String> {
        let post_state = maybe_post_state
            .or(self.post_state_hash)
            .expect("builder must have a post-state hash");

        let query_request = QueryRequest::new(post_state, base_key, path.to_vec());

        let query_result = self
            .engine_state
            .run_query(CorrelationId::new(), query_request)
            .expect("should get query response");

        if let QueryResult::Success { value, .. } = query_result {
            return Ok(value.deref().clone());
        }

        Err(format!("{:?}", query_result))
    }

    /// Queries state for a dictionary item.
    pub fn query_dictionary_item(
        &self,
        maybe_post_state: Option<Digest>,
        dictionary_seed_uref: URef,
        dictionary_item_key: &str,
    ) -> Result<StoredValue, String> {
        let dictionary_address =
            Key::dictionary(dictionary_seed_uref, dictionary_item_key.as_bytes());
        let empty_path: Vec<String> = vec![];
        self.query(maybe_post_state, dictionary_address, &empty_path)
    }

    /// Queries for a [`StoredValue`] and returns the [`StoredValue`] and a Merkle proof.
    pub fn query_with_proof(
        &self,
        maybe_post_state: Option<Digest>,
        base_key: Key,
        path: &[String],
    ) -> Result<(StoredValue, Vec<TrieMerkleProof<Key, StoredValue>>), String> {
        let post_state = maybe_post_state
            .or(self.post_state_hash)
            .expect("builder must have a post-state hash");

        let path_vec: Vec<String> = path.to_vec();

        let query_request = QueryRequest::new(post_state, base_key, path_vec);

        let query_result = self
            .engine_state
            .run_query(CorrelationId::new(), query_request)
            .expect("should get query response");

        if let QueryResult::Success { value, proofs } = query_result {
            return Ok((value.deref().clone(), proofs));
        }

        panic! {"{:?}", query_result};
    }

    /// Queries for the total supply of token.
    /// # Panics
    /// Panics if the total supply can't be found.
    pub fn total_supply(&self, maybe_post_state: Option<Digest>) -> U512 {
        let mint_key: Key = self
            .get_system_contract_hash(MINT)
            .cloned()
            .expect("should have mint_contract_hash")
            .into();

        let result = self.query(maybe_post_state, mint_key, &[TOTAL_SUPPLY_KEY.to_string()]);

        let total_supply: U512 = if let Ok(StoredValue::CLValue(total_supply)) = result {
            total_supply.into_t().expect("total supply should be U512")
        } else {
            panic!("mint should track total supply");
        };

        total_supply
    }

    /// Queries for the base round reward.
    /// # Panics
    /// Panics if the total supply or seigniorage rate can't be found.
    pub fn base_round_reward(&mut self, maybe_post_state: Option<Digest>) -> U512 {
        let mint_key: Key = self.get_mint_contract_hash().into();

        let mint_contract = self
            .query(maybe_post_state, mint_key, &[])
            .expect("must get mint stored value")
            .as_contract()
            .expect("must convert to mint contract")
            .clone();

        let mint_named_keys = mint_contract.named_keys().clone();

        let total_supply_uref = *mint_named_keys
            .get(TOTAL_SUPPLY_KEY)
            .expect("must track total supply")
            .as_uref()
            .expect("must get uref");

        let round_seigniorage_rate_uref = *mint_named_keys
            .get(ROUND_SEIGNIORAGE_RATE_KEY)
            .expect("must track round seigniorage rate");

        let total_supply = self
            .query(maybe_post_state, Key::URef(total_supply_uref), &[])
            .expect("must read value under total supply URef")
            .as_cl_value()
            .expect("must convert into CL value")
            .clone()
            .into_t::<U512>()
            .expect("must convert into U512");

        let rate = self
            .query(maybe_post_state, round_seigniorage_rate_uref, &[])
            .expect("must read value")
            .as_cl_value()
            .expect("must conver to cl value")
            .clone()
            .into_t::<Ratio<U512>>()
            .expect("must conver to ratio");

        rate.checked_mul(&Ratio::from(total_supply))
            .map(|ratio| ratio.to_integer())
            .expect("must get base round reward")
    }

    /// Runs an [`ExecuteRequest`].
    pub fn exec(&mut self, mut exec_request: ExecuteRequest) -> &mut Self {
        let exec_request = {
            let hash = self.post_state_hash.expect("expected post_state_hash");
            exec_request.parent_state_hash = hash;
            exec_request
        };

        let maybe_exec_results = self
            .engine_state
            .run_execute(CorrelationId::new(), exec_request);
        assert!(maybe_exec_results.is_ok());
        // Parse deploy results
        let execution_results = maybe_exec_results.as_ref().unwrap();
        // Cache transformations
        self.transforms.extend(
            execution_results
                .iter()
                .map(|res| res.execution_journal().clone()),
        );
        self.exec_results.push(
            maybe_exec_results
                .unwrap()
                .into_iter()
                .map(Rc::new)
                .collect(),
        );
        self
    }

    /// Commit effects of previous exec call on the latest post-state hash.
    pub fn commit(&mut self) -> &mut Self {
        let prestate_hash = self.post_state_hash.expect("Should have genesis hash");

        let effects = self.transforms.last().cloned().unwrap_or_default();

        self.commit_transforms(prestate_hash, effects.into())
    }

    /// Runs a commit request, expects a successful response, and
    /// overwrites existing cached post state hash with a new one.
    pub fn commit_transforms(
        &mut self,
        pre_state_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> &mut Self {
        let post_state_hash = self
            .engine_state
            .apply_effect(CorrelationId::new(), pre_state_hash, effects)
            .expect("should commit");
        self.post_state_hash = Some(post_state_hash);
        self
    }

    /// Upgrades the execution engine.
    pub fn upgrade_with_upgrade_request(
        &mut self,
        engine_config: EngineConfig,
        upgrade_config: &mut UpgradeConfig,
    ) -> &mut Self {
        let pre_state_hash = self.post_state_hash.expect("should have state hash");
        upgrade_config.with_pre_state_hash(pre_state_hash);

        let engine_state = Rc::get_mut(&mut self.engine_state).unwrap();
        engine_state.update_config(engine_config);

        let result = self
            .engine_state
            .commit_upgrade(CorrelationId::new(), upgrade_config.clone());

        if let Ok(UpgradeSuccess {
            post_state_hash,
            execution_effect: _,
        }) = result
        {
            self.post_state_hash = Some(post_state_hash);

            if let Ok(StoredValue::CLValue(cl_registry)) =
                self.query(self.post_state_hash, Key::SystemContractRegistry, &[])
            {
                let registry = CLValue::into_t::<SystemContractRegistry>(cl_registry).unwrap();
                self.system_contract_registry = Some(registry);
            }
        }

        self.upgrade_results.push(result);
        self
    }

    /// Executes a request to call the system auction contract.
    pub fn run_auction(
        &mut self,
        era_end_timestamp_millis: u64,
        evicted_validators: Vec<PublicKey>,
    ) -> &mut Self {
        let auction = self.get_auction_contract_hash();
        let run_request = ExecuteRequestBuilder::contract_call_by_hash(
            *SYSTEM_ADDR,
            auction,
            METHOD_RUN_AUCTION,
            runtime_args! {
                ARG_ERA_END_TIMESTAMP_MILLIS => era_end_timestamp_millis,
                ARG_EVICTED_VALIDATORS => evicted_validators,
            },
        )
        .build();
        self.exec(run_request).commit().expect_success()
    }

    /// Increments engine state.
    pub fn step(&mut self, step_request: StepRequest) -> Result<StepSuccess, StepError> {
        let step_result = self
            .engine_state
            .commit_step(CorrelationId::new(), step_request);

        if let Ok(StepSuccess {
            post_state_hash, ..
        }) = &step_result
        {
            self.post_state_hash = Some(*post_state_hash);
        }

        step_result
    }

    /// Expects a successful run
    pub fn expect_success(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let exec_results = self
            .get_last_exec_results()
            .expect("Expected to be called after run()");
        let exec_result = exec_results
            .get(0)
            .expect("Unable to get first deploy result");

        if exec_result.is_failure() {
            panic!(
                "Expected successful execution result, but instead got: {:#?}",
                exec_result,
            );
        }
        self
    }

    /// Expects a failed run
    pub fn expect_failure(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let exec_results = self
            .get_last_exec_results()
            .expect("Expected to be called after run()");
        let exec_result = exec_results
            .get(0)
            .expect("Unable to get first deploy result");

        if exec_result.is_success() {
            panic!(
                "Expected failed execution result, but instead got: {:?}",
                exec_result,
            );
        }

        self
    }

    /// Returns `true` if the las exec had an error, otherwise returns false.
    pub fn is_error(&self) -> bool {
        self.get_last_exec_results()
            .expect("Expected to be called after run()")
            .get(0)
            .expect("Unable to get first execution result")
            .is_failure()
    }

    /// Returns an `Option<engine_state::Error>` if the last exec had an error.
    pub fn get_error(&self) -> Option<engine_state::Error> {
        self.get_last_exec_results()
            .expect("Expected to be called after run()")
            .get(0)
            .expect("Unable to get first deploy result")
            .as_error()
            .cloned()
    }

    /// Gets the transform map that's cached between runs
    #[deprecated(
        since = "2.1.0",
        note = "Use `get_execution_journals` that returns transforms in the order they were created."
    )]
    pub fn get_transforms(&self) -> Vec<AdditiveMap<Key, Transform>> {
        self.transforms
            .clone()
            .into_iter()
            .map(|journal| journal.into_iter().collect())
            .collect()
    }

    /// Gets `ExecutionJournal`s of all passed runs.
    pub fn get_execution_journals(&self) -> Vec<ExecutionJournal> {
        self.transforms.clone()
    }

    /// Gets genesis account (if present)
    pub fn get_genesis_account(&self) -> &Account {
        self.genesis_account
            .as_ref()
            .expect("Unable to obtain genesis account. Please run genesis first.")
    }

    /// Returns the [`ContractHash`] of the mint, panics if it can't be found.
    pub fn get_mint_contract_hash(&self) -> ContractHash {
        self.get_system_contract_hash(MINT)
            .cloned()
            .expect("Unable to obtain mint contract. Please run genesis first.")
    }

    /// Returns the [`ContractHash`] of the "handle payment" contract, panics if it can't be found.
    pub fn get_handle_payment_contract_hash(&self) -> ContractHash {
        self.get_system_contract_hash(HANDLE_PAYMENT)
            .cloned()
            .expect("Unable to obtain handle payment contract. Please run genesis first.")
    }

    /// Returns the [`ContractHash`] of the "standard payment" contract, panics if it can't be
    /// found.
    pub fn get_standard_payment_contract_hash(&self) -> ContractHash {
        self.get_system_contract_hash(STANDARD_PAYMENT)
            .cloned()
            .expect("Unable to obtain standard payment contract. Please run genesis first.")
    }

    fn get_system_contract_hash(&self, contract_name: &str) -> Option<&ContractHash> {
        self.system_contract_registry
            .as_ref()
            .and_then(|registry| registry.get(contract_name))
    }

    /// Returns the [`ContractHash`] of the "auction" contract, panics if it can't be found.
    pub fn get_auction_contract_hash(&self) -> ContractHash {
        self.get_system_contract_hash(AUCTION)
            .cloned()
            .expect("Unable to obtain auction contract. Please run genesis first.")
    }

    /// Returns genesis transforms, panics if there aren't any.
    pub fn get_genesis_transforms(&self) -> &AdditiveMap<Key, Transform> {
        self.genesis_transforms
            .as_ref()
            .expect("should have genesis transforms")
    }

    /// Returns the genesis hash, panics if it can't be found.
    pub fn get_genesis_hash(&self) -> Digest {
        self.genesis_hash
            .expect("Genesis hash should be present. Should be called after run_genesis.")
    }

    /// Returns the post state hash, panics if it can't be found.
    pub fn get_post_state_hash(&self) -> Digest {
        self.post_state_hash.expect("Should have post-state hash.")
    }

    /// Returns the engine state.
    pub fn get_engine_state(&self) -> &EngineState<S> {
        &self.engine_state
    }

    /// Returns the last results execs.
    pub fn get_last_exec_results(&self) -> Option<Vec<Rc<ExecutionResult>>> {
        let exec_results = self.exec_results.last()?;

        Some(exec_results.iter().map(Rc::clone).collect())
    }

    /// Returns the results of a specific exec.
    pub fn get_exec_result(&self, index: usize) -> Option<Vec<Rc<ExecutionResult>>> {
        let exec_results = self.exec_results.get(index)?;

        Some(exec_results.iter().map(Rc::clone).collect())
    }

    /// Returns a count of exec results.
    pub fn get_exec_results_count(&self) -> usize {
        self.exec_results.len()
    }

    /// Returns a `Result` containing an [`UpgradeSuccess`].
    pub fn get_upgrade_result(
        &self,
        index: usize,
    ) -> Option<&Result<UpgradeSuccess, engine_state::Error>> {
        self.upgrade_results.get(index)
    }

    /// Expects upgrade success.
    pub fn expect_upgrade_success(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let result = self
            .upgrade_results
            .last()
            .expect("Expected to be called after a system upgrade.")
            .as_ref();

        result.unwrap_or_else(|_| panic!("Expected success, got: {:?}", result));

        self
    }

    /// Returns the "handle payment" contract, panics if it can't be found.
    pub fn get_handle_payment_contract(&self) -> Contract {
        let handle_payment_contract: Key = self
            .get_system_contract_hash(HANDLE_PAYMENT)
            .cloned()
            .expect("should have handle payment contract uref")
            .into();
        self.query(None, handle_payment_contract, &[])
            .and_then(|v| v.try_into().map_err(|error| format!("{:?}", error)))
            .expect("should find handle payment URef")
    }

    /// Returns the balance of a purse, panics if the balance can't be parsed into a `U512`.
    pub fn get_purse_balance(&self, purse: URef) -> U512 {
        let base_key = Key::Balance(purse.addr());
        self.query(None, base_key, &[])
            .and_then(|v| CLValue::try_from(v).map_err(|error| format!("{:?}", error)))
            .and_then(|cl_value| cl_value.into_t().map_err(|error| format!("{:?}", error)))
            .expect("should parse balance into a U512")
    }

    /// Returns a `BalanceResult` for a purse, panics if the balance can't be found.
    pub fn get_purse_balance_result(&self, purse: URef) -> BalanceResult {
        let correlation_id = CorrelationId::new();
        let state_root_hash: Digest = self.post_state_hash.expect("should have post_state_hash");
        self.engine_state
            .get_purse_balance(correlation_id, state_root_hash, purse)
            .expect("should get purse balance")
    }

    /// Returns a `BalanceResult` for a purse using a `PublicKey`.
    pub fn get_public_key_balance_result(&self, public_key: PublicKey) -> BalanceResult {
        let correlation_id = CorrelationId::new();
        let state_root_hash: Digest = self.post_state_hash.expect("should have post_state_hash");
        self.engine_state
            .get_balance(correlation_id, state_root_hash, public_key)
            .expect("should get purse balance using public key")
    }

    /// Gets the purse balance of a proposer.
    pub fn get_proposer_purse_balance(&self) -> U512 {
        let proposer_account = self
            .get_account(*DEFAULT_PROPOSER_ADDR)
            .expect("proposer account should exist");
        self.get_purse_balance(proposer_account.main_purse())
    }

    /// Queries for an `Account`.
    pub fn get_account(&self, account_hash: AccountHash) -> Option<Account> {
        match self.query(None, Key::Account(account_hash), &[]) {
            Ok(account_value) => match account_value {
                StoredValue::Account(account) => Some(account),
                _ => None,
            },
            Err(_) => None,
        }
    }

    /// Queries for an `Account` and panics if it can't be found.
    pub fn get_expected_account(&self, account_hash: AccountHash) -> Account {
        self.get_account(account_hash).expect("account to exist")
    }

    /// Queries for a contract by `ContractHash`.
    pub fn get_contract(&self, contract_hash: ContractHash) -> Option<Contract> {
        let contract_value: StoredValue = self
            .query(None, contract_hash.into(), &[])
            .expect("should have contract value");

        if let StoredValue::Contract(contract) = contract_value {
            Some(contract)
        } else {
            None
        }
    }

    /// Queries for a contract by `ContractHash` and returns an `Option<ContractWasm>`.
    pub fn get_contract_wasm(&self, contract_hash: ContractHash) -> Option<ContractWasm> {
        let contract_value: StoredValue = self
            .query(None, contract_hash.into(), &[])
            .expect("should have contract value");

        if let StoredValue::ContractWasm(contract_wasm) = contract_value {
            Some(contract_wasm)
        } else {
            None
        }
    }

    /// Queries for a contract package by `ContractPackageHash`.
    pub fn get_contract_package(
        &self,
        contract_package_hash: ContractPackageHash,
    ) -> Option<ContractPackage> {
        let contract_value: StoredValue = self
            .query(None, contract_package_hash.into(), &[])
            .expect("should have package value");

        if let StoredValue::ContractPackage(package) = contract_value {
            Some(package)
        } else {
            None
        }
    }

    /// Queries for a transfer by `TransferAddr`.
    pub fn get_transfer(&self, transfer: TransferAddr) -> Option<Transfer> {
        let transfer_value: StoredValue = self
            .query(None, Key::Transfer(transfer), &[])
            .expect("should have transfer value");

        if let StoredValue::Transfer(transfer) = transfer_value {
            Some(transfer)
        } else {
            None
        }
    }

    /// Queries for deploy info by `DeployHash`.
    pub fn get_deploy_info(&self, deploy_hash: DeployHash) -> Option<DeployInfo> {
        let deploy_info_value: StoredValue = self
            .query(None, Key::DeployInfo(deploy_hash), &[])
            .expect("should have deploy info value");

        if let StoredValue::DeployInfo(deploy_info) = deploy_info_value {
            Some(deploy_info)
        } else {
            None
        }
    }

    /// Returns a `Vec<Gas>` representing execution consts.
    pub fn exec_costs(&self, index: usize) -> Vec<Gas> {
        let exec_results = self
            .get_exec_result(index)
            .expect("should have exec response");
        utils::get_exec_costs(exec_results)
    }

    /// Returns the `Gas` cost of the last exec.
    pub fn last_exec_gas_cost(&self) -> Gas {
        let exec_results = self
            .get_last_exec_results()
            .expect("Expected to be called after run()");
        let exec_result = exec_results.get(0).expect("should have result");
        exec_result.cost()
    }

    /// Returns the result of the last exec.
    pub fn last_exec_result(&self) -> &ExecutionResult {
        let exec_results = self
            .exec_results
            .last()
            .expect("Expected to be called after run()");
        exec_results.get(0).expect("should have result").as_ref()
    }

    /// Assert that last error is the expected one.
    ///
    /// NOTE: we're using string-based representation for checking equality
    /// as the `Error` type does not implement `Eq` (many of its subvariants don't).
    pub fn assert_error(&self, expected_error: Error) {
        match self.get_error() {
            Some(error) => assert_eq!(format!("{:?}", expected_error), format!("{:?}", error)),
            None => panic!("expected error ({:?}) got success", expected_error),
        }
    }

    /// Returns the error message of the last exec.
    pub fn exec_error_message(&self, index: usize) -> Option<String> {
        let response = self.get_exec_result(index)?;
        Some(utils::get_error_message(response))
    }

    /// Gets [`EraValidators`].
    pub fn get_era_validators(&mut self) -> EraValidators {
        let correlation_id = CorrelationId::new();
        let state_hash = self.get_post_state_hash();
        let request = GetEraValidatorsRequest::new(state_hash, *DEFAULT_PROTOCOL_VERSION);
        let system_contract_registry = self
            .system_contract_registry
            .clone()
            .expect("System contract registry not found. Please run genesis first.");
        self.engine_state
            .get_era_validators(correlation_id, Some(system_contract_registry), request)
            .expect("get era validators should not error")
    }

    /// Gets [`ValidatorWeights`] for a given [`EraId`].
    pub fn get_validator_weights(&mut self, era_id: EraId) -> Option<ValidatorWeights> {
        let mut result = self.get_era_validators();
        result.remove(&era_id)
    }

    /// Gets [`Bids`].
    pub fn get_bids(&mut self) -> Bids {
        let get_bids_request = GetBidsRequest::new(self.get_post_state_hash());

        let get_bids_result = self
            .engine_state
            .get_bids(CorrelationId::new(), get_bids_request)
            .unwrap();

        get_bids_result.into_success().unwrap()
    }

    /// Gets [`UnbondingPurses`].
    pub fn get_unbonds(&mut self) -> UnbondingPurses {
        let correlation_id = CorrelationId::new();
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .engine_state
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let reader = tracking_copy.reader();

        let unbond_keys = reader
            .keys_with_prefix(correlation_id, &[KeyTag::Unbond as u8])
            .unwrap_or_default();

        let mut ret = BTreeMap::new();

        for key in unbond_keys.into_iter() {
            let read_result = reader.read(correlation_id, &key);
            if let (Key::Unbond(account_hash), Ok(Some(StoredValue::Unbonding(unbonding_purses)))) =
                (key, read_result)
            {
                ret.insert(account_hash, unbonding_purses);
            }
        }

        ret
    }

    /// Gets [`WithdrawPurses`].
    pub fn get_withdraws(&mut self) -> WithdrawPurses {
        let correlation_id = CorrelationId::new();
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .engine_state
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let reader = tracking_copy.reader();

        let withdraws_keys = reader
            .keys_with_prefix(correlation_id, &[KeyTag::Withdraw as u8])
            .unwrap_or_default();

        let mut ret = BTreeMap::new();

        for key in withdraws_keys.into_iter() {
            let read_result = reader.read(correlation_id, &key);
            if let (Key::Withdraw(account_hash), Ok(Some(StoredValue::Withdraw(withdraw_purses)))) =
                (key, read_result)
            {
                ret.insert(account_hash, withdraw_purses);
            }
        }

        ret
    }

    /// Gets all `[Key::Balance]`s in global state.
    pub fn get_balance_keys(&mut self) -> Vec<Key> {
        let correlation_id = CorrelationId::new();
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .engine_state
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let reader = tracking_copy.reader();

        reader
            .keys_with_prefix(correlation_id, &[KeyTag::Balance as u8])
            .unwrap_or_default()
    }

    /// Gets a stored value from a contract's named keys.
    pub fn get_value<T>(&mut self, contract_hash: ContractHash, name: &str) -> T
    where
        T: FromBytes + CLTyped,
    {
        let contract = self
            .get_contract(contract_hash)
            .expect("should have contract");
        let key = contract
            .named_keys()
            .get(name)
            .expect("should have named key");
        let stored_value = self.query(None, *key, &[]).expect("should query");
        let cl_value = stored_value
            .as_cl_value()
            .cloned()
            .expect("should be cl value");
        let result: T = cl_value.into_t().expect("should convert");
        result
    }

    /// Gets an [`EraId`].
    pub fn get_era(&mut self) -> EraId {
        let auction_contract = self.get_auction_contract_hash();
        self.get_value(auction_contract, ERA_ID_KEY)
    }

    /// Gets the auction delay.
    pub fn get_auction_delay(&mut self) -> u64 {
        let auction_contract = self.get_auction_contract_hash();
        self.get_value(auction_contract, AUCTION_DELAY_KEY)
    }

    /// Gets the unbonding delay
    pub fn get_unbonding_delay(&mut self) -> u64 {
        let auction_contract = self.get_auction_contract_hash();
        self.get_value(auction_contract, UNBONDING_DELAY_KEY)
    }

    /// Gets the [`ContractHash`] of the system auction contract, panics if it can't be found.
    pub fn get_system_auction_hash(&self) -> ContractHash {
        let correlation_id = CorrelationId::new();
        let state_root_hash = self.get_post_state_hash();
        self.engine_state
            .get_system_auction_hash(correlation_id, state_root_hash)
            .expect("should have auction hash")
    }

    /// Gets the [`ContractHash`] of the system mint contract, panics if it can't be found.
    pub fn get_system_mint_hash(&self) -> ContractHash {
        let correlation_id = CorrelationId::new();
        let state_root_hash = self.get_post_state_hash();
        self.engine_state
            .get_system_mint_hash(correlation_id, state_root_hash)
            .expect("should have auction hash")
    }

    /// Gets the [`ContractHash`] of the system handle payment contract, panics if it can't be
    /// found.
    pub fn get_system_handle_payment_hash(&self) -> ContractHash {
        let correlation_id = CorrelationId::new();
        let state_root_hash = self.get_post_state_hash();
        self.engine_state
            .get_handle_payment_hash(correlation_id, state_root_hash)
            .expect("should have handle payment hash")
    }

    /// Returns the [`ContractHash`] of the system standard payment contract, panics if it can't be
    /// found.
    pub fn get_system_standard_payment_hash(&self) -> ContractHash {
        let correlation_id = CorrelationId::new();
        let state_root_hash = self.get_post_state_hash();
        self.engine_state
            .get_standard_payment_hash(correlation_id, state_root_hash)
            .expect("should have standard payment hash")
    }

    /// Resets the `exec_results`, `upgrade_results` and `transform` fields.
    pub fn clear_results(&mut self) -> &mut Self {
        self.exec_results = Vec::new();
        self.upgrade_results = Vec::new();
        self.transforms = Vec::new();
        self
    }

    /// Advances eras by num_eras
    pub fn advance_eras_by(
        &mut self,
        num_eras: u64,
        reward_items: impl IntoIterator<Item = RewardItem>,
    ) {
        let step_request_builder = StepRequestBuilder::new()
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_reward_items(reward_items)
            .with_run_auction(true);

        for _ in 0..num_eras {
            let step_request = step_request_builder
                .clone()
                .with_parent_state_hash(self.get_post_state_hash())
                .with_next_era_id(self.get_era().successor())
                .build();

            self.step(step_request)
                .expect("failed to execute step request");
        }
    }

    /// Advances eras by configured amount
    pub fn advance_eras_by_default_auction_delay(
        &mut self,
        reward_items: impl IntoIterator<Item = RewardItem>,
    ) {
        let auction_delay = self.get_auction_delay();
        self.advance_eras_by(auction_delay + 1, reward_items);
    }

    /// Advancess by a single era.
    pub fn advance_era(&mut self, reward_items: impl IntoIterator<Item = RewardItem>) {
        self.advance_eras_by(1, reward_items);
    }

    /// Returns a trie by hash.
    pub fn get_trie(&mut self, state_hash: Digest) -> Option<Trie<Key, StoredValue>> {
        self.engine_state
            .get_trie_full(CorrelationId::default(), state_hash)
            .unwrap()
            .map(|bytes| bytesrepr::deserialize(bytes.into()).unwrap())
    }

    /// Returns the costs related to interacting with the auction system contract.
    pub fn get_auction_costs(&self) -> AuctionCosts {
        *self.engine_state.config().system_config().auction_costs()
    }

    /// Returns the costs related to interacting with the mint system contract.
    pub fn get_mint_costs(&self) -> MintCosts {
        *self.engine_state.config().system_config().mint_costs()
    }

    /// Returns the costs related to interacting with the handle payment system contract.
    pub fn get_handle_payment_costs(&self) -> HandlePaymentCosts {
        *self
            .engine_state
            .config()
            .system_config()
            .handle_payment_costs()
    }
}
