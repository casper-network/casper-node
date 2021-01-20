use std::{
    convert::{TryFrom, TryInto},
    ffi::OsStr,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};

use bytesrepr::FromBytes;
use grpc::RequestOptions;
use lmdb::DatabaseFlags;
use log::LevelFilter;

use casper_engine_grpc_server::engine_server::{
    ipc::{
        CommitRequest, CommitResponse, GenesisResponse, QueryRequest, StepRequest, UpgradeRequest,
        UpgradeResponse,
    },
    ipc_grpc::ExecutionEngineService,
    mappings::TransformMap,
    transforms::TransformEntry,
};
use casper_execution_engine::{
    core::{
        engine_state::{
            era_validators::GetEraValidatorsRequest, execute_request::ExecuteRequest,
            execution_result::ExecutionResult, run_genesis_request::RunGenesisRequest,
            BalanceResult, EngineConfig, EngineState, SYSTEM_ACCOUNT_ADDR,
        },
        execution,
    },
    shared::{
        account::Account,
        additive_map::AdditiveMap,
        gas::Gas,
        logging::{self, Settings, Style},
        newtypes::{Blake2bHash, CorrelationId},
        stored_value::StoredValue,
        transform::Transform,
        utils::OS_PAGE_SIZE,
    },
    storage::{
        global_state::{in_memory::InMemoryGlobalState, lmdb::LmdbGlobalState, StateProvider},
        protocol_data_store::lmdb::LmdbProtocolDataStore,
        transaction_source::lmdb::LmdbEnvironment,
        trie::merkle_proof::TrieMerkleProof,
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_types::{
    account::AccountHash,
    auction::{
        EraId, EraValidators, ValidatorWeights, AUCTION_DELAY_KEY, ERA_ID_KEY, METHOD_RUN_AUCTION,
    },
    bytesrepr::{self},
    mint::TOTAL_SUPPLY_KEY,
    runtime_args, CLTyped, CLValue, Contract, ContractHash, ContractWasm, DeployHash, DeployInfo,
    Key, RuntimeArgs, Transfer, TransferAddr, URef, U512,
};

use crate::internal::{
    utils, ExecuteRequestBuilder, DEFAULT_PROPOSER_ADDR, DEFAULT_PROTOCOL_VERSION,
};

/// LMDB initial map size is calculated based on DEFAULT_LMDB_PAGES and systems page size.
///
/// This default value should give 50MiB initial map size by default.
const DEFAULT_LMDB_PAGES: usize = 128_000;

/// LDMB max readers
///
/// The default value is chosen to be the same as the node itself.
const DEFAULT_MAX_READERS: u32 = 512;

/// This is appended to the data dir path provided to the `LmdbWasmTestBuilder` in order to match
/// the behavior of `get_data_dir()` in "engine-grpc-server/src/main.rs".
const GLOBAL_STATE_DIR: &str = "global_state";

pub type InMemoryWasmTestBuilder = WasmTestBuilder<InMemoryGlobalState>;
pub type LmdbWasmTestBuilder = WasmTestBuilder<LmdbGlobalState>;

/// Builder for simple WASM test
pub struct WasmTestBuilder<S> {
    /// [`EngineState`] is wrapped in [`Rc`] to work around a missing [`Clone`] implementation
    engine_state: Rc<EngineState<S>>,
    /// [`ExecutionResult`] is wrapped in [`Rc`] to work around a missing [`Clone`] implementation
    exec_responses: Vec<Vec<Rc<ExecutionResult>>>,
    upgrade_responses: Vec<UpgradeResponse>,
    genesis_hash: Option<Vec<u8>>,
    post_state_hash: Option<Vec<u8>>,
    /// Cached transform maps after subsequent successful runs i.e. `transforms[0]` is for first
    /// exec call etc.
    transforms: Vec<AdditiveMap<Key, Transform>>,
    /// Cached genesis transforms
    genesis_account: Option<Account>,
    /// Genesis transforms
    genesis_transforms: Option<AdditiveMap<Key, Transform>>,
    /// Mint contract key
    mint_contract_hash: Option<ContractHash>,
    /// PoS contract key
    pos_contract_hash: Option<ContractHash>,
    /// Standard payment contract key
    standard_payment_hash: Option<ContractHash>,
    /// Auction contract key
    auction_contract_hash: Option<ContractHash>,
}

impl<S> WasmTestBuilder<S> {
    fn initialize_logging() {
        let log_settings = Settings::new(LevelFilter::Error).with_style(Style::HumanReadable);
        let _ = logging::initialize(log_settings);
    }
}

impl Default for InMemoryWasmTestBuilder {
    fn default() -> Self {
        Self::initialize_logging();
        let engine_config = EngineConfig::new();

        let global_state = InMemoryGlobalState::empty().expect("should create global state");
        let engine_state = EngineState::new(global_state, engine_config);

        WasmTestBuilder {
            engine_state: Rc::new(engine_state),
            exec_responses: Vec::new(),
            upgrade_responses: Vec::new(),
            genesis_hash: None,
            post_state_hash: None,
            transforms: Vec::new(),
            genesis_account: None,
            genesis_transforms: None,
            mint_contract_hash: None,
            pos_contract_hash: None,
            standard_payment_hash: None,
            auction_contract_hash: None,
        }
    }
}

// TODO: Deriving `Clone` for `WasmTestBuilder<S>` doesn't work correctly (unsure why), so
// implemented by hand here.  Try to derive in the future with a different compiler version.
impl<S> Clone for WasmTestBuilder<S> {
    fn clone(&self) -> Self {
        WasmTestBuilder {
            engine_state: Rc::clone(&self.engine_state),
            exec_responses: self.exec_responses.clone(),
            upgrade_responses: self.upgrade_responses.clone(),
            genesis_hash: self.genesis_hash.clone(),
            post_state_hash: self.post_state_hash.clone(),
            transforms: self.transforms.clone(),
            genesis_account: self.genesis_account.clone(),
            genesis_transforms: self.genesis_transforms.clone(),
            mint_contract_hash: self.mint_contract_hash,
            pos_contract_hash: self.pos_contract_hash,
            standard_payment_hash: self.standard_payment_hash,
            auction_contract_hash: self.auction_contract_hash,
        }
    }
}

/// A wrapper type to disambiguate builder from an actual result
#[derive(Clone)]
pub struct WasmTestResult<S>(WasmTestBuilder<S>);

impl<S> WasmTestResult<S> {
    /// Access the builder
    pub fn builder(&self) -> &WasmTestBuilder<S> {
        &self.0
    }
}

impl InMemoryWasmTestBuilder {
    pub fn new(
        global_state: InMemoryGlobalState,
        engine_config: EngineConfig,
        post_state_hash: Vec<u8>,
    ) -> Self {
        Self::initialize_logging();
        let engine_state = EngineState::new(global_state, engine_config);
        WasmTestBuilder {
            engine_state: Rc::new(engine_state),
            genesis_hash: Some(post_state_hash.clone()),
            post_state_hash: Some(post_state_hash),
            ..Default::default()
        }
    }
}

impl LmdbWasmTestBuilder {
    pub fn new_with_config<T: AsRef<OsStr> + ?Sized>(
        data_dir: &T,
        engine_config: EngineConfig,
    ) -> Self {
        Self::initialize_logging();
        let page_size = *OS_PAGE_SIZE;
        let global_state_dir = Self::create_and_get_global_state_dir(data_dir);
        let environment = Arc::new(
            LmdbEnvironment::new(
                &global_state_dir,
                page_size * DEFAULT_LMDB_PAGES,
                DEFAULT_MAX_READERS,
            )
            .expect("should create LmdbEnvironment"),
        );
        let trie_store = Arc::new(
            LmdbTrieStore::new(&environment, None, DatabaseFlags::empty())
                .expect("should create LmdbTrieStore"),
        );
        let protocol_data_store = Arc::new(
            LmdbProtocolDataStore::new(&environment, None, DatabaseFlags::empty())
                .expect("should create LmdbProtocolDataStore"),
        );
        let global_state = LmdbGlobalState::empty(environment, trie_store, protocol_data_store)
            .expect("should create LmdbGlobalState");
        let engine_state = EngineState::new(global_state, engine_config);
        WasmTestBuilder {
            engine_state: Rc::new(engine_state),
            exec_responses: Vec::new(),
            upgrade_responses: Vec::new(),
            genesis_hash: None,
            post_state_hash: None,
            transforms: Vec::new(),
            genesis_account: None,
            genesis_transforms: None,
            mint_contract_hash: None,
            pos_contract_hash: None,
            standard_payment_hash: None,
            auction_contract_hash: None,
        }
    }

    pub fn new<T: AsRef<OsStr> + ?Sized>(data_dir: &T) -> Self {
        Self::new_with_config(data_dir, Default::default())
    }

    /// Creates new instance of builder and applies values only which allows the engine state to be
    /// swapped with a new one, possibly after running genesis once and reusing existing database
    /// (i.e. LMDB).
    pub fn new_with_config_and_result<T: AsRef<OsStr> + ?Sized>(
        data_dir: &T,
        engine_config: EngineConfig,
        result: &WasmTestResult<LmdbGlobalState>,
    ) -> Self {
        let mut builder = Self::new_with_config(data_dir, engine_config);
        // Applies existing properties from gi
        builder.genesis_hash = result.0.genesis_hash.clone();
        builder.post_state_hash = result.0.post_state_hash.clone();
        builder.mint_contract_hash = result.0.mint_contract_hash;
        builder.pos_contract_hash = result.0.pos_contract_hash;
        builder
    }

    /// Creates a new instance of builder using the supplied configurations, opening wrapped LMDBs
    /// (e.g. in the Trie and Data stores) rather than creating them.
    pub fn open<T: AsRef<OsStr> + ?Sized>(
        data_dir: &T,
        engine_config: EngineConfig,
        post_state_hash: Vec<u8>,
    ) -> Self {
        Self::initialize_logging();
        let page_size = *OS_PAGE_SIZE;
        let global_state_dir = Self::create_and_get_global_state_dir(data_dir);
        let environment = Arc::new(
            LmdbEnvironment::new(
                &global_state_dir,
                page_size * DEFAULT_LMDB_PAGES,
                DEFAULT_MAX_READERS,
            )
            .expect("should create LmdbEnvironment"),
        );
        let trie_store =
            Arc::new(LmdbTrieStore::open(&environment, None).expect("should open LmdbTrieStore"));
        let protocol_data_store = Arc::new(
            LmdbProtocolDataStore::open(&environment, None)
                .expect("should open LmdbProtocolDataStore"),
        );
        let global_state = LmdbGlobalState::empty(environment, trie_store, protocol_data_store)
            .expect("should create LmdbGlobalState");
        let engine_state = EngineState::new(global_state, engine_config);
        WasmTestBuilder {
            engine_state: Rc::new(engine_state),
            exec_responses: Vec::new(),
            upgrade_responses: Vec::new(),
            genesis_hash: None,
            post_state_hash: Some(post_state_hash),
            transforms: Vec::new(),
            genesis_account: None,
            genesis_transforms: None,
            mint_contract_hash: None,
            pos_contract_hash: None,
            standard_payment_hash: None,
            auction_contract_hash: None,
        }
    }

    fn create_and_get_global_state_dir<T: AsRef<OsStr> + ?Sized>(data_dir: &T) -> PathBuf {
        let global_state_path = {
            let mut path = PathBuf::from(data_dir);
            path.push(GLOBAL_STATE_DIR);
            path
        };
        fs::create_dir_all(&global_state_path)
            .unwrap_or_else(|_| panic!("Expected to create {}", global_state_path.display()));
        global_state_path
    }
}

impl<S> WasmTestBuilder<S>
where
    S: StateProvider,
    S::Error: Into<execution::Error>,
    EngineState<S>: ExecutionEngineService,
{
    /// Carries on attributes from TestResult for further executions
    pub fn from_result(result: WasmTestResult<S>) -> Self {
        WasmTestBuilder {
            engine_state: result.0.engine_state,
            exec_responses: Vec::new(),
            upgrade_responses: Vec::new(),
            genesis_hash: result.0.genesis_hash,
            post_state_hash: result.0.post_state_hash,
            transforms: Vec::new(),
            genesis_account: result.0.genesis_account,
            mint_contract_hash: result.0.mint_contract_hash,
            pos_contract_hash: result.0.pos_contract_hash,
            standard_payment_hash: result.0.standard_payment_hash,
            auction_contract_hash: result.0.auction_contract_hash,
            genesis_transforms: result.0.genesis_transforms,
        }
    }

    pub fn run_genesis(&mut self, run_genesis_request: &RunGenesisRequest) -> &mut Self {
        let system_account = Key::Account(SYSTEM_ACCOUNT_ADDR);
        let run_genesis_request_proto = run_genesis_request
            .to_owned()
            .try_into()
            .expect("could not parse");

        let genesis_response = self
            .engine_state
            .run_genesis(RequestOptions::new(), run_genesis_request_proto)
            .wait_drop_metadata()
            .expect("Unable to get genesis response");
        if genesis_response.has_failed_deploy() {
            panic!(
                "genesis failure: {:?}",
                genesis_response.get_failed_deploy().to_owned()
            );
        }

        let state_root_hash: Blake2bHash = genesis_response
            .get_success()
            .get_poststate_hash()
            .try_into()
            .expect("Unable to get root hash");

        let transforms = get_genesis_transforms(&genesis_response);

        let genesis_account =
            utils::get_account(&transforms, &system_account).expect("Unable to get system account");

        let maybe_protocol_data = self
            .engine_state
            .get_protocol_data(run_genesis_request.protocol_version())
            .expect("should read protocol data");
        let protocol_data = maybe_protocol_data.expect("should have protocol data stored");

        self.genesis_hash = Some(state_root_hash.to_vec());
        self.post_state_hash = Some(state_root_hash.to_vec());
        self.mint_contract_hash = Some(protocol_data.mint());
        self.pos_contract_hash = Some(protocol_data.proof_of_stake());
        self.standard_payment_hash = Some(protocol_data.standard_payment());
        self.auction_contract_hash = Some(protocol_data.auction());
        self.genesis_account = Some(genesis_account);
        self.genesis_transforms = Some(transforms);
        self
    }

    pub fn query(
        &self,
        maybe_post_state: Option<Vec<u8>>,
        base_key: Key,
        path: &[&str],
    ) -> Result<StoredValue, String> {
        let post_state = maybe_post_state
            .or_else(|| self.post_state_hash.clone())
            .expect("builder must have a post-state hash");

        let path_vec: Vec<String> = path.iter().map(|s| String::from(*s)).collect();

        let query_request = create_query_request(post_state, base_key, path_vec);

        let mut query_response = self
            .engine_state
            .query(RequestOptions::new(), query_request)
            .wait_drop_metadata()
            .expect("should get query response");

        if query_response.has_failure() {
            return Err(query_response.take_failure());
        }

        let (value, _proofs): (StoredValue, Vec<TrieMerkleProof<Key, StoredValue>>) =
            bytesrepr::deserialize(query_response.take_success())
                .map_err(|err| format!("{:?}", err))?;

        Ok(value)
    }

    pub fn query_with_proof(
        &self,
        maybe_post_state: Option<Vec<u8>>,
        base_key: Key,
        path: &[String],
    ) -> Result<(StoredValue, Vec<TrieMerkleProof<Key, StoredValue>>), String> {
        let post_state = maybe_post_state
            .or_else(|| self.post_state_hash.clone())
            .expect("builder must have a post-state hash");

        let path_vec: Vec<String> = path.to_vec();

        let query_request = create_query_request(post_state, base_key, path_vec);

        let mut query_response = self
            .engine_state
            .query(RequestOptions::new(), query_request)
            .wait_drop_metadata()
            .expect("should get query response");

        if query_response.has_failure() {
            return Err(query_response.take_failure());
        }

        let (value, proofs): (StoredValue, Vec<TrieMerkleProof<Key, StoredValue>>) =
            bytesrepr::deserialize(query_response.take_success())
                .map_err(|err| format!("{:?}", err))?;

        Ok((value, proofs))
    }

    pub fn total_supply(&self, maybe_post_state: Option<Vec<u8>>) -> U512 {
        let mint_key: Key = self
            .mint_contract_hash
            .expect("should have mint_contract_hash")
            .into();

        let result = self.query(maybe_post_state, mint_key, &[TOTAL_SUPPLY_KEY]);

        let total_supply: U512 = if let Ok(StoredValue::CLValue(total_supply)) = result {
            total_supply.into_t().expect("total supply should be U512")
        } else {
            panic!("mint should track total supply");
        };

        total_supply
    }

    pub fn exec(&mut self, mut exec_request: ExecuteRequest) -> &mut Self {
        let exec_request = {
            let hash = self
                .post_state_hash
                .clone()
                .expect("expected post_state_hash");
            exec_request.parent_state_hash =
                hash.as_slice().try_into().expect("expected a valid hash");
            exec_request
        };
        let exec_response = self
            .engine_state
            .run_execute(CorrelationId::new(), exec_request);
        assert!(exec_response.is_ok());
        // Parse deploy results
        let execution_results = exec_response.as_ref().unwrap();
        // Cache transformations
        self.transforms.extend(
            execution_results
                .iter()
                .map(|res| res.effect().transforms.clone()),
        );
        self.exec_responses
            .push(exec_response.unwrap().into_iter().map(Rc::new).collect());
        self
    }

    /// Commit effects of previous exec call on the latest post-state hash.
    pub fn commit(&mut self) -> &mut Self {
        let prestate_hash = self
            .post_state_hash
            .clone()
            .expect("Should have genesis hash");

        let effects = self.transforms.last().cloned().unwrap_or_default();

        self.commit_effects(prestate_hash, effects)
    }

    /// Sends raw commit request to the current engine response.
    ///
    /// Can be used where result is not necessary
    pub fn commit_transforms(
        &self,
        prestate_hash: Vec<u8>,
        effects: AdditiveMap<Key, Transform>,
    ) -> CommitResponse {
        let commit_request = create_commit_request(&prestate_hash, &effects);

        self.engine_state
            .commit(RequestOptions::new(), commit_request)
            .wait_drop_metadata()
            .expect("Should have commit response")
    }

    /// Runs a commit request, expects a successful response, and
    /// overwrites existing cached post state hash with a new one.
    pub fn commit_effects(
        &mut self,
        prestate_hash: Vec<u8>,
        effects: AdditiveMap<Key, Transform>,
    ) -> &mut Self {
        let mut commit_response = self.commit_transforms(prestate_hash, effects);
        if !commit_response.has_success() {
            panic!(
                "Expected commit success but received a failure instead: {:?}",
                commit_response
            );
        }
        let mut commit_success = commit_response.take_success();
        self.post_state_hash = Some(commit_success.take_poststate_hash().to_vec());
        self
    }

    pub fn upgrade_with_upgrade_request(
        &mut self,
        upgrade_request: &mut UpgradeRequest,
    ) -> &mut Self {
        let upgrade_request = {
            let hash = self
                .post_state_hash
                .clone()
                .expect("expected post_state_hash");
            upgrade_request.set_parent_state_hash(hash.to_vec());
            upgrade_request
        };
        let upgrade_response = self
            .engine_state
            .upgrade(RequestOptions::new(), upgrade_request.clone())
            .wait_drop_metadata()
            .expect("should upgrade");

        let upgrade_success = upgrade_response.get_success();
        self.post_state_hash = Some(upgrade_success.get_post_state_hash().to_vec());

        self.upgrade_responses.push(upgrade_response.clone());
        self
    }

    pub fn run_auction(&mut self) -> &mut Self {
        const ARG_ENTRY_POINT: &str = "entry_point";
        const SYSTEM_ADDR: AccountHash = AccountHash::new([0u8; 32]);
        const CONTRACT_AUCTION_BIDS: &str = "auction_bids.wasm";
        let run_request = ExecuteRequestBuilder::standard(
            SYSTEM_ADDR,
            CONTRACT_AUCTION_BIDS,
            runtime_args! {
                ARG_ENTRY_POINT => METHOD_RUN_AUCTION
            },
        )
        .build();
        self.exec(run_request).commit().expect_success()
    }

    pub fn step(&mut self, step_request: StepRequest) -> &mut Self {
        let response = self
            .engine_state
            .step(RequestOptions::new(), step_request)
            .wait_drop_metadata()
            .expect("should step");

        if !response.has_step_result() {
            panic!("Expected step result");
        }

        let result = response.get_step_result();

        if result.has_success() {
            let success = result.get_success();
            self.post_state_hash = Some(success.get_poststate_hash().to_vec());
            return self;
        } else if result.has_error() {
            let error = result.get_error();
            panic!(
                "Expected successful step result, but instead got error: {:?}",
                error,
            );
        } else if result.has_missing_parent() {
            let missing_parent = result.get_missing_parent();
            panic!(
                "Expected successful step result, but instead got missing_parent: {:?}",
                missing_parent,
            );
        }
        panic!("Expected one of the supported step result options (this should be unreachable).");
    }

    /// Expects a successful run and caches transformations
    pub fn expect_success(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let exec_response = self
            .exec_responses
            .last()
            .expect("Expected to be called after run()");
        let exec_result = exec_response
            .get(0)
            .expect("Unable to get first deploy result");

        if exec_result.is_failure() {
            panic!(
                "Expected successful execution result, but instead got: {:?}",
                exec_response,
            );
        }
        self
    }

    pub fn is_error(&self) -> bool {
        let exec_response = self
            .exec_responses
            .last()
            .expect("Expected to be called after run()");
        let exec_result = exec_response
            .get(0)
            .expect("Unable to get first execution result");
        exec_result.is_failure()
    }

    /// Gets the transform map that's cached between runs
    pub fn get_transforms(&self) -> Vec<AdditiveMap<Key, Transform>> {
        self.transforms.clone()
    }

    /// Gets genesis account (if present)
    pub fn get_genesis_account(&self) -> &Account {
        self.genesis_account
            .as_ref()
            .expect("Unable to obtain genesis account. Please run genesis first.")
    }

    pub fn get_mint_contract_hash(&self) -> ContractHash {
        self.mint_contract_hash
            .expect("Unable to obtain mint contract. Please run genesis first.")
    }

    pub fn get_pos_contract_hash(&self) -> ContractHash {
        self.pos_contract_hash
            .expect("Unable to obtain pos contract. Please run genesis first.")
    }

    pub fn get_standard_payment_contract_hash(&self) -> ContractHash {
        self.standard_payment_hash
            .expect("Unable to obtain standard payment contract. Please run genesis first.")
    }

    pub fn get_auction_contract_hash(&self) -> ContractHash {
        self.auction_contract_hash
            .expect("Unable to obtain auction contract. Please run genesis first.")
    }

    pub fn get_genesis_transforms(&self) -> &AdditiveMap<Key, Transform> {
        &self
            .genesis_transforms
            .as_ref()
            .expect("should have genesis transforms")
    }

    pub fn get_genesis_hash(&self) -> Vec<u8> {
        self.genesis_hash
            .clone()
            .expect("Genesis hash should be present. Should be called after run_genesis.")
    }

    pub fn get_post_state_hash(&self) -> Vec<u8> {
        self.post_state_hash
            .clone()
            .expect("Should have post-state hash.")
    }

    pub fn get_engine_state(&self) -> &EngineState<S> {
        &self.engine_state
    }

    pub fn get_exec_responses(&self) -> &Vec<Vec<Rc<ExecutionResult>>> {
        &self.exec_responses
    }

    pub fn get_exec_response(&self, index: usize) -> Option<&Vec<Rc<ExecutionResult>>> {
        self.exec_responses.get(index)
    }

    pub fn get_exec_responses_count(&self) -> usize {
        self.exec_responses.len()
    }

    pub fn get_upgrade_response(&self, index: usize) -> Option<&UpgradeResponse> {
        self.upgrade_responses.get(index)
    }

    pub fn finish(&self) -> WasmTestResult<S> {
        WasmTestResult(self.clone())
    }

    pub fn get_pos_contract(&self) -> Contract {
        let pos_contract: Key = self
            .pos_contract_hash
            .expect("should have pos contract uref")
            .into();
        self.query(None, pos_contract, &[])
            .and_then(|v| v.try_into().map_err(|error| format!("{:?}", error)))
            .expect("should find PoS URef")
    }

    pub fn get_purse_balance(&self, purse: URef) -> U512 {
        let purse_addr = purse.addr();
        let balance_mapping_key = Key::Hash(purse_addr);

        let base_key = self
            .query(None, balance_mapping_key, &[])
            .and_then(|v| CLValue::try_from(v).map_err(|error| format!("{:?}", error)))
            .and_then(|cl_value| cl_value.into_t().map_err(|error| format!("{:?}", error)))
            .expect("should find balance uref");

        self.query(None, base_key, &[])
            .and_then(|v| CLValue::try_from(v).map_err(|error| format!("{:?}", error)))
            .and_then(|cl_value| cl_value.into_t().map_err(|error| format!("{:?}", error)))
            .expect("should parse balance into a U512")
    }

    pub fn get_purse_balance_result(&self, purse: URef) -> BalanceResult {
        let correlation_id = CorrelationId::new();
        let state_root_hash_slice: &Vec<u8> = self
            .post_state_hash
            .as_ref()
            .expect("should have post_state_hash");
        let state_root_hash: Blake2bHash = state_root_hash_slice
            .as_slice()
            .try_into()
            .expect("should convert");
        self.engine_state
            .get_purse_balance(correlation_id, state_root_hash, purse)
            .expect("should get purse balance")
    }

    pub fn get_proposer_purse_balance(&self) -> U512 {
        let proposer_account = self
            .get_account(*DEFAULT_PROPOSER_ADDR)
            .expect("proposer account should exist");
        self.get_purse_balance(proposer_account.main_purse())
    }

    pub fn get_account(&self, account_hash: AccountHash) -> Option<Account> {
        match self.query(None, Key::Account(account_hash), &[]) {
            Ok(account_value) => match account_value {
                StoredValue::Account(account) => Some(account),
                _ => None,
            },
            Err(_) => None,
        }
    }

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

    pub fn exec_costs(&self, index: usize) -> Vec<Gas> {
        let exec_response = self
            .get_exec_response(index)
            .expect("should have exec response");
        utils::get_exec_costs(exec_response)
    }

    pub fn last_exec_gas_cost(&self) -> Gas {
        let exec_response = self
            .exec_responses
            .last()
            .expect("Expected to be called after run()");
        let exec_result = exec_response.get(0).expect("should have result");
        exec_result.cost()
    }

    pub fn exec_error_message(&self, index: usize) -> Option<String> {
        let response = self.get_exec_response(index)?;
        Some(utils::get_error_message(response))
    }

    pub fn exec_commit_finish(&mut self, execute_request: ExecuteRequest) -> WasmTestResult<S> {
        self.exec(execute_request)
            .expect_success()
            .commit()
            .finish()
    }

    pub fn get_era_validators(&mut self) -> EraValidators {
        let correlation_id = CorrelationId::new();
        let state_hash = Blake2bHash::try_from(self.get_post_state_hash().as_slice())
            .expect("should create state hash");
        let request = GetEraValidatorsRequest::new(state_hash, *DEFAULT_PROTOCOL_VERSION);
        self.engine_state
            .get_era_validators(correlation_id, request)
            .expect("get era validators should not error")
    }

    pub fn get_validator_weights(&mut self, era_id: EraId) -> Option<ValidatorWeights> {
        let mut result = self.get_era_validators();
        result.remove(&era_id)
    }

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

    pub fn get_era(&mut self) -> EraId {
        let auction_contract = self.get_auction_contract_hash();
        self.get_value(auction_contract, ERA_ID_KEY)
    }

    pub fn get_auction_delay(&mut self) -> u64 {
        let auction_contract = self.get_auction_contract_hash();
        self.get_value(auction_contract, AUCTION_DELAY_KEY)
    }
}

fn create_query_request(post_state: Vec<u8>, base_key: Key, path: Vec<String>) -> QueryRequest {
    let mut query_request = QueryRequest::new();

    query_request.set_state_hash(post_state);
    query_request.set_base_key(base_key.into());
    query_request.set_path(path.into());

    query_request
}

#[allow(clippy::implicit_hasher)]
fn create_commit_request(
    prestate_hash: &[u8],
    effects: &AdditiveMap<Key, Transform>,
) -> CommitRequest {
    let effects: Vec<TransformEntry> = effects
        .iter()
        .map(|(k, t)| (k.to_owned(), t.to_owned()).into())
        .collect();

    let mut commit_request = CommitRequest::new();
    commit_request.set_prestate_hash(prestate_hash.to_vec());
    commit_request.set_effects(effects.into());
    commit_request
}

#[allow(clippy::implicit_hasher)]
fn get_genesis_transforms(genesis_response: &GenesisResponse) -> AdditiveMap<Key, Transform> {
    let commit_transforms: TransformMap = genesis_response
        .get_success()
        .get_effect()
        .get_transform_map()
        .to_vec()
        .try_into()
        .expect("should convert");
    commit_transforms.into_inner()
}
