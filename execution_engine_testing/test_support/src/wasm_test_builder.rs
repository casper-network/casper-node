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
use num_rational::Ratio;
use num_traits::CheckedMul;

use casper_execution_engine::engine_state::{
    execute_request::ExecuteRequest,
    execution_result::ExecutionResult,
    step::{StepRequest, StepSuccess},
    EngineConfig, EngineConfigBuilder, EngineState, Error, PruneConfig, PruneResult, StepError,
    DEFAULT_MAX_QUERY_DEPTH,
};
use casper_storage::{
    data_access_layer::{
        BalanceResult, BidsRequest, BlockStore, DataAccessLayer, EraValidatorsRequest,
        EraValidatorsResult, GenesisRequest, GenesisResult, ProtocolUpgradeRequest,
        ProtocolUpgradeResult, QueryRequest, QueryResult,
    },
    global_state::{
        state::{
            lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, StateProvider,
            StateReader,
        },
        transaction_source::lmdb::LmdbEnvironment,
        trie::Trie,
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{EntityKindTag, NamedKeyAddr, NamedKeys},
    bytesrepr::{self, FromBytes},
    contracts::ContractHash,
    execution::Effects,
    global_state::TrieMerkleProof,
    runtime_args,
    system::{
        auction::{
            BidKind, EraValidators, UnbondingPurse, UnbondingPurses, ValidatorWeights,
            WithdrawPurses, ARG_ERA_END_TIMESTAMP_MILLIS, ARG_EVICTED_VALIDATORS,
            AUCTION_DELAY_KEY, ERA_ID_KEY, METHOD_RUN_AUCTION, UNBONDING_DELAY_KEY,
        },
        mint::{ROUND_SEIGNIORAGE_RATE_KEY, TOTAL_SUPPLY_KEY},
        AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
    },
    AddressableEntity, AddressableEntityHash, AuctionCosts, ByteCode, ByteCodeAddr, ByteCodeHash,
    CLTyped, CLValue, Contract, DeployHash, DeployInfo, Digest, EntityAddr, EraId, Gas,
    HandlePaymentCosts, Key, KeyTag, MintCosts, Motes, Package, PackageHash, ProtocolUpgradeConfig,
    ProtocolVersion, PublicKey, RefundHandling, StoredValue, SystemEntityRegistry, Transfer,
    TransferAddr, URef, OS_PAGE_SIZE, U512,
};
use tempfile::TempDir;

use crate::{
    chainspec_config::{ChainspecConfig, PRODUCTION_PATH},
    utils, ExecuteRequestBuilder, StepRequestBuilder, DEFAULT_GAS_PRICE, DEFAULT_PROPOSER_ADDR,
    DEFAULT_PROTOCOL_VERSION, SYSTEM_ADDR,
};

/// LMDB initial map size is calculated based on DEFAULT_LMDB_PAGES and systems page size.
pub(crate) const DEFAULT_LMDB_PAGES: usize = 256_000_000;

/// LMDB max readers
///
/// The default value is chosen to be the same as the node itself.
pub(crate) const DEFAULT_MAX_READERS: u32 = 512;

/// This is appended to the data dir path provided to the `LmdbWasmTestBuilder`".
const GLOBAL_STATE_DIR: &str = "global_state";

/// A wrapper structure that groups an entity alongside its namedkeys.
pub struct EntityWithNamedKeys {
    entity: AddressableEntity,
    named_keys: NamedKeys,
}

impl EntityWithNamedKeys {
    /// Creates a new instance of an Entity with its NamedKeys.
    pub fn new(entity: AddressableEntity, named_keys: NamedKeys) -> Self {
        Self { entity, named_keys }
    }

    /// Returns a reference to the Entity.
    pub fn entity(&self) -> AddressableEntity {
        self.entity.clone()
    }

    /// Returns a reference to the main purse for the inner entity.
    pub fn main_purse(&self) -> URef {
        self.entity.main_purse()
    }

    /// Returns a reference to the NamedKeys.
    pub fn named_keys(&self) -> &NamedKeys {
        &self.named_keys
    }
}

/// Wasm test builder where state is held in LMDB.
pub type LmdbWasmTestBuilder = WasmTestBuilder<DataAccessLayer<LmdbGlobalState>>;

/// Wasm test builder where Lmdb state is held in a automatically cleaned up temporary directory.
// pub type TempLmdbWasmTestBuilder = WasmTestBuilder<TemporaryLmdbGlobalState>;

/// Builder for simple WASM test
pub struct WasmTestBuilder<S> {
    /// [`EngineState`] is wrapped in [`Rc`] to work around a missing [`Clone`] implementation.
    engine_state: Rc<EngineState<S>>,
    /// [`ExecutionResult`] is wrapped in [`Rc`] to work around a missing [`Clone`] implementation.
    exec_results: Vec<Vec<Rc<ExecutionResult>>>,
    upgrade_results: Vec<ProtocolUpgradeResult>,
    prune_results: Vec<Result<PruneResult, Error>>,
    genesis_hash: Option<Digest>,
    /// Post state hash.
    post_state_hash: Option<Digest>,
    /// Cached effects after successful runs i.e. `effects[0]` is the collection of effects for
    /// first exec call, etc.
    effects: Vec<Effects>,
    /// Genesis effects.
    genesis_effects: Option<Effects>,
    /// Cached system account.
    system_account: Option<AddressableEntity>,
    /// Scratch global state used for in-memory execution and commit optimization.
    scratch_engine_state: Option<EngineState<ScratchGlobalState>>,
    /// Global state dir, for implementations that define one.
    global_state_dir: Option<PathBuf>,
    /// Temporary directory, for implementation that uses one.
    temp_dir: Option<Rc<TempDir>>,
}

impl Default for LmdbWasmTestBuilder {
    fn default() -> Self {
        Self::new_temporary_with_chainspec(&*PRODUCTION_PATH)
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
            prune_results: self.prune_results.clone(),
            genesis_hash: self.genesis_hash,
            post_state_hash: self.post_state_hash,
            effects: self.effects.clone(),
            genesis_effects: self.genesis_effects.clone(),
            system_account: self.system_account.clone(),
            scratch_engine_state: None,
            global_state_dir: self.global_state_dir.clone(),
            temp_dir: self.temp_dir.clone(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum GlobalStateMode {
    /// Creates empty lmdb database with specified flags
    Create(DatabaseFlags),
    /// Opens existing database
    Open(Digest),
}

impl GlobalStateMode {
    fn post_state_hash(self) -> Option<Digest> {
        match self {
            GlobalStateMode::Create(_) => None,
            GlobalStateMode::Open(post_state_hash) => Some(post_state_hash),
        }
    }
}

impl LmdbWasmTestBuilder {
    /// Upgrades the execution engine using the scratch trie.
    pub fn upgrade_using_scratch(
        &mut self,
        engine_config: EngineConfig,
        upgrade_config: &mut ProtocolUpgradeConfig,
    ) -> &mut Self {
        let pre_state_hash = self.post_state_hash.expect("should have state hash");
        upgrade_config.with_pre_state_hash(pre_state_hash);

        let engine_state = Rc::get_mut(&mut self.engine_state).unwrap();
        engine_state.update_config(engine_config);

        let scratch_state = self.engine_state.get_scratch_engine_state();
        let pre_state_hash = upgrade_config.pre_state_hash();
        let req = ProtocolUpgradeRequest::new(upgrade_config.clone());
        let result = {
            let result = scratch_state.commit_upgrade(req);
            if let ProtocolUpgradeResult::Success { effects, .. } = result {
                let post_state_hash = self
                    .engine_state
                    .write_scratch_to_db(pre_state_hash, scratch_state.into_inner())
                    .unwrap();
                self.post_state_hash = Some(post_state_hash);
                ProtocolUpgradeResult::Success {
                    post_state_hash,
                    effects,
                }
            } else {
                result
            }
        };
        self.upgrade_results.push(result);
        self
    }

    /// Returns an [`LmdbWasmTestBuilder`] with configuration.
    pub fn new_with_config<T: AsRef<OsStr> + ?Sized>(
        data_dir: &T,
        engine_config: EngineConfig,
    ) -> Self {
        let _ = env_logger::try_init();
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

        let max_query_depth = DEFAULT_MAX_QUERY_DEPTH;
        let global_state = LmdbGlobalState::empty(environment, trie_store, max_query_depth)
            .expect("should create LmdbGlobalState");

        let data_access_layer = DataAccessLayer {
            block_store: BlockStore::new(),
            state: global_state,
            max_query_depth,
        };
        let engine_state = EngineState::new(data_access_layer, engine_config);

        WasmTestBuilder {
            engine_state: Rc::new(engine_state),
            exec_results: Vec::new(),
            upgrade_results: Vec::new(),
            prune_results: Vec::new(),
            genesis_hash: None,
            post_state_hash: None,
            effects: Vec::new(),
            system_account: None,
            genesis_effects: None,
            scratch_engine_state: None,
            global_state_dir: Some(global_state_dir),
            temp_dir: None,
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

        let engine_config = EngineConfigBuilder::new()
            .with_max_query_depth(DEFAULT_MAX_QUERY_DEPTH)
            .with_max_associated_keys(chainspec_config.core_config.max_associated_keys)
            .with_max_runtime_call_stack_height(
                chainspec_config.core_config.max_runtime_call_stack_height,
            )
            .with_minimum_delegation_amount(chainspec_config.core_config.minimum_delegation_amount)
            .with_strict_argument_checking(chainspec_config.core_config.strict_argument_checking)
            .with_vesting_schedule_period_millis(
                chainspec_config
                    .core_config
                    .vesting_schedule_period
                    .millis(),
            )
            .with_max_delegators_per_validator(
                chainspec_config.core_config.max_delegators_per_validator,
            )
            .with_wasm_config(chainspec_config.wasm_config)
            .with_system_config(chainspec_config.system_costs_config)
            .build();

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

    fn create_or_open<T: AsRef<Path>>(
        global_state_dir: T,
        engine_config: EngineConfig,
        mode: GlobalStateMode,
    ) -> Self {
        let _ = env_logger::try_init();
        let page_size = *OS_PAGE_SIZE;

        match mode {
            GlobalStateMode::Create(_database_flags) => {}
            GlobalStateMode::Open(_post_state_hash) => {
                Self::create_global_state_dir(&global_state_dir)
            }
        }

        let environment = LmdbEnvironment::new(
            &global_state_dir,
            page_size * DEFAULT_LMDB_PAGES,
            DEFAULT_MAX_READERS,
            true,
        )
        .expect("should create LmdbEnvironment");

        let max_query_depth = DEFAULT_MAX_QUERY_DEPTH;

        let global_state = match mode {
            GlobalStateMode::Create(database_flags) => {
                let trie_store = LmdbTrieStore::new(&environment, None, database_flags)
                    .expect("should open LmdbTrieStore");
                LmdbGlobalState::empty(Arc::new(environment), Arc::new(trie_store), max_query_depth)
                    .expect("should create LmdbGlobalState")
            }
            GlobalStateMode::Open(post_state_hash) => {
                let trie_store =
                    LmdbTrieStore::open(&environment, None).expect("should open LmdbTrieStore");
                LmdbGlobalState::new(
                    Arc::new(environment),
                    Arc::new(trie_store),
                    post_state_hash,
                    max_query_depth,
                )
            }
        };

        let data_access_layer = DataAccessLayer {
            block_store: BlockStore::new(),
            state: global_state,
            max_query_depth,
        };

        let engine_state = EngineState::new(data_access_layer, engine_config);

        let post_state_hash = mode.post_state_hash();

        let builder = WasmTestBuilder {
            engine_state: Rc::new(engine_state),
            exec_results: Vec::new(),
            upgrade_results: Vec::new(),
            prune_results: Vec::new(),
            genesis_hash: None,
            post_state_hash,
            effects: Vec::new(),
            genesis_effects: None,
            system_account: None,
            scratch_engine_state: None,
            global_state_dir: Some(global_state_dir.as_ref().to_path_buf()),
            temp_dir: None,
        };

        builder
    }

    /// Creates a new instance of builder using the supplied configurations, opening wrapped LMDBs
    /// (e.g. in the Trie and Data stores) rather than creating them.
    /// Differs from `open` in that it doesn't append `GLOBAL_STATE_DIR` to the supplied path.
    pub fn open_raw<T: AsRef<Path>>(
        global_state_dir: T,
        engine_config: EngineConfig,
        post_state_hash: Digest,
    ) -> Self {
        Self::create_or_open(
            global_state_dir,
            engine_config,
            GlobalStateMode::Open(post_state_hash),
        )
    }

    /// Creates new temporary lmdb builder with an engine config instance.
    ///
    /// Once [`LmdbWasmTestBuilder`] instance goes out of scope a global state directory will be
    /// removed as well.
    pub fn new_temporary_with_config(engine_config: EngineConfig) -> Self {
        let temp_dir = tempfile::tempdir().unwrap();

        let database_flags = DatabaseFlags::default();

        let mut builder = Self::create_or_open(
            temp_dir.path(),
            engine_config,
            GlobalStateMode::Create(database_flags),
        );

        builder.temp_dir = Some(Rc::new(temp_dir));

        builder
    }

    /// Creates new temporary lmdb builder with a path to a chainspec to load.
    ///
    /// Once [`LmdbWasmTestBuilder`] instance goes out of scope a global state directory will be
    /// removed as well.
    pub fn new_temporary_with_chainspec<P: AsRef<Path>>(chainspec_path: P) -> Self {
        let chainspec_config = ChainspecConfig::from_chainspec_path(chainspec_path)
            .expect("must build chainspec configuration");

        let vesting_schedule_period_millis = chainspec_config
            .core_config
            .vesting_schedule_period
            .millis();

        let engine_config = EngineConfigBuilder::new()
            .with_max_query_depth(DEFAULT_MAX_QUERY_DEPTH)
            .with_max_associated_keys(chainspec_config.core_config.max_associated_keys)
            .with_max_runtime_call_stack_height(
                chainspec_config.core_config.max_runtime_call_stack_height,
            )
            .with_minimum_delegation_amount(chainspec_config.core_config.minimum_delegation_amount)
            .with_strict_argument_checking(chainspec_config.core_config.strict_argument_checking)
            .with_vesting_schedule_period_millis(vesting_schedule_period_millis)
            .with_max_delegators_per_validator(
                chainspec_config.core_config.max_delegators_per_validator,
            )
            .with_wasm_config(chainspec_config.wasm_config)
            .with_system_config(chainspec_config.system_costs_config)
            .build();

        Self::new_temporary_with_config(engine_config)
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

    /// Returns the file size on disk of the backing lmdb file behind LmdbGlobalState.
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
        let maybe_exec_results = cached_state.run_execute(exec_request);
        for execution_result in maybe_exec_results.unwrap() {
            let _post_state_hash = cached_state
                .commit_effects(
                    self.post_state_hash.expect("requires a post_state_hash"),
                    execution_result.effects().clone(),
                )
                .expect("should commit");

            // Save transforms and execution results for WasmTestBuilder.
            self.effects.push(execution_result.effects().clone());
            exec_results.push(Rc::new(execution_result))
        }
        self.exec_results.push(exec_results);
        self
    }

    /// Commit scratch to global state, and reset the scratch cache.
    pub fn write_scratch_to_db(&mut self) -> &mut Self {
        let prestate_hash = self.post_state_hash.expect("Should have genesis hash");
        if let Some(scratch) = self.scratch_engine_state.take() {
            let new_state_root = self
                .engine_state
                .write_scratch_to_db(prestate_hash, scratch.into_inner())
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
            .commit_step(step_request)
            .expect("unable to run step request against scratch global state");
        self
    }
}

impl<S> WasmTestBuilder<S>
where
    S: StateProvider + CommitProvider,
{
    /// Takes a [`GenesisRequest`], executes the request and returns Self.
    pub fn run_genesis(&mut self, request: GenesisRequest) -> &mut Self {
        match self.engine_state.commit_genesis(request) {
            GenesisResult::Fatal(msg) => {
                panic!("{}", msg);
            }
            GenesisResult::Failure(err) => {
                panic!("{:?}", err);
            }
            GenesisResult::Success {
                post_state_hash,
                effects,
            } => {
                self.genesis_hash = Some(post_state_hash);
                self.post_state_hash = Some(post_state_hash);
                self.system_account = self.get_entity_by_account_hash(*SYSTEM_ADDR);
                self.genesis_effects = Some(effects);
            }
        }
        self
    }

    fn query_system_contract_registry(
        &self,
        post_state_hash: Option<Digest>,
    ) -> Option<SystemEntityRegistry> {
        match self.query(post_state_hash, Key::SystemEntityRegistry, &[]) {
            Ok(StoredValue::CLValue(cl_registry)) => {
                let system_contract_registry =
                    CLValue::into_t::<SystemEntityRegistry>(cl_registry).unwrap();
                Some(system_contract_registry)
            }
            Ok(_) => None,
            Err(_) => None,
        }
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

        let query_result = self.engine_state.run_query(query_request);
        if let QueryResult::Success { value, .. } = query_result {
            return Ok(value.deref().clone());
        }

        Err(format!("{:?}", query_result))
    }

    /// Query a named key in global state by account hash.
    pub fn query_named_key_by_account_hash(
        &self,
        maybe_post_state: Option<Digest>,
        account_hash: AccountHash,
        name: &str,
    ) -> Result<StoredValue, String> {
        let entity_addr = self
            .get_entity_hash_by_account_hash(account_hash)
            .map(|entity_hash| EntityAddr::new_account_entity_addr(entity_hash.value()))
            .expect("must get EntityAddr");
        self.query_named_key(maybe_post_state, entity_addr, name)
    }

    /// Query a named key.
    pub fn query_named_key(
        &self,
        maybe_post_state: Option<Digest>,
        entity_addr: EntityAddr,
        name: &str,
    ) -> Result<StoredValue, String> {
        let named_key_addr = NamedKeyAddr::new_from_string(entity_addr, name.to_string())
            .expect("could not create named key address");
        let empty_path: Vec<String> = vec![];
        let maybe_stored_value = self
            .query(maybe_post_state, Key::NamedKey(named_key_addr), &empty_path)
            .expect("no stored value found");
        let key = maybe_stored_value
            .as_cl_value()
            .map(|cl_val| CLValue::into_t::<Key>(cl_val.clone()))
            .expect("must be cl_value")
            .expect("must get key");
        self.query(maybe_post_state, key, &[])
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

        let query_result = self.engine_state.run_query(query_request);

        if let QueryResult::Success { value, proofs } = query_result {
            return Ok((value.deref().clone(), proofs));
        }

        panic! {"{:?}", query_result};
    }

    /// Queries for the total supply of token.
    /// # Panics
    /// Panics if the total supply can't be found.
    pub fn total_supply(&self, maybe_post_state: Option<Digest>) -> U512 {
        let mint_entity_hash = self
            .get_system_entity_hash(MINT)
            .expect("should have mint_contract_hash");

        let mint_key = Key::addressable_entity_key(EntityKindTag::System, mint_entity_hash);

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
        let mint_named_keys =
            self.get_named_keys(EntityAddr::System(self.get_mint_contract_hash().value()));

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
            .into_cl_value()
            .expect("must convert into CL value")
            .into_t::<U512>()
            .expect("must convert into U512");

        let rate = self
            .query(maybe_post_state, round_seigniorage_rate_uref, &[])
            .expect("must read value")
            .into_cl_value()
            .expect("must conver to cl value")
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

        let maybe_exec_results = self.engine_state.run_execute(exec_request);
        assert!(maybe_exec_results.is_ok());
        // Parse deploy results
        let execution_results = maybe_exec_results.as_ref().unwrap();
        // Cache transformations
        self.effects
            .extend(execution_results.iter().map(|res| res.effects().clone()));
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

        let effects = self.effects.last().cloned().unwrap_or_default();

        self.commit_transforms(prestate_hash, effects)
    }

    /// Runs a commit request, expects a successful response, and
    /// overwrites existing cached post state hash with a new one.
    pub fn commit_transforms(&mut self, pre_state_hash: Digest, effects: Effects) -> &mut Self {
        let post_state_hash = self
            .engine_state
            .commit_effects(pre_state_hash, effects)
            .expect("should commit");
        self.post_state_hash = Some(post_state_hash);
        self
    }

    /// Upgrades the execution engine.
    /// Deprecated - this path does not use the scratch trie and generates many interstitial commits
    /// on upgrade.
    pub fn upgrade_with_upgrade_request(
        &mut self,
        engine_config: EngineConfig,
        upgrade_config: &mut ProtocolUpgradeConfig,
    ) -> &mut Self {
        self.upgrade_with_upgrade_request_and_config(Some(engine_config), upgrade_config)
    }

    /// Upgrades the execution engine.
    ///
    /// If `engine_config` is set to None, then it is defaulted to the current one.
    pub fn upgrade_with_upgrade_request_and_config(
        &mut self,
        engine_config: Option<EngineConfig>,
        upgrade_config: &mut ProtocolUpgradeConfig,
    ) -> &mut Self {
        let engine_config = engine_config.unwrap_or_else(|| self.engine_state.config().clone());

        let pre_state_hash = self.post_state_hash.expect("should have state hash");
        upgrade_config.with_pre_state_hash(pre_state_hash);

        let engine_state_mut =
            Rc::get_mut(&mut self.engine_state).expect("should have unique ownership");
        engine_state_mut.update_config(engine_config);

        let req = ProtocolUpgradeRequest::new(upgrade_config.clone());
        let result = self.engine_state.commit_upgrade(req);

        if let ProtocolUpgradeResult::Success {
            post_state_hash, ..
        } = result
        {
            self.post_state_hash = Some(post_state_hash);
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
        self.exec(run_request).expect_success().commit()
    }

    /// Increments engine state.
    pub fn step(&mut self, step_request: StepRequest) -> Result<StepSuccess, StepError> {
        let step_result = self.engine_state.commit_step(step_request);

        if let Ok(StepSuccess {
            post_state_hash, ..
        }) = &step_result
        {
            self.post_state_hash = Some(*post_state_hash);
        }

        step_result
    }

    /// Distributes the rewards.
    pub fn distribute(
        &mut self,
        pre_state_hash: Option<Digest>,
        protocol_version: ProtocolVersion,
        rewards: &BTreeMap<PublicKey, U512>,
        next_block_height: u64,
        time: u64,
    ) -> Result<Digest, StepError> {
        let pre_state_hash = pre_state_hash.or(self.post_state_hash).unwrap();
        let post_state_hash = self.engine_state.distribute_block_rewards(
            pre_state_hash,
            protocol_version,
            rewards,
            next_block_height,
            time,
        )?;

        self.post_state_hash = Some(post_state_hash);

        Ok(post_state_hash)
    }

    /// Expects a successful run
    #[track_caller]
    pub fn expect_success(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let exec_results = self
            .get_last_exec_result()
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
            .get_last_exec_result()
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
        self.get_last_exec_result()
            .expect("Expected to be called after run()")
            .get(0)
            .expect("Unable to get first execution result")
            .is_failure()
    }

    /// Returns an `Option<engine_state::Error>` if the last exec had an error.
    pub fn get_error(&self) -> Option<Error> {
        self.get_last_exec_result()
            .expect("Expected to be called after run()")
            .get(0)
            .expect("Unable to get first deploy result")
            .as_error()
            .cloned()
    }

    /// Gets `Effects` of all previous runs.
    pub fn get_effects(&self) -> Vec<Effects> {
        self.effects.clone()
    }

    /// Gets genesis account (if present)
    pub fn get_genesis_account(&self) -> &AddressableEntity {
        self.system_account
            .as_ref()
            .expect("Unable to obtain genesis account. Please run genesis first.")
    }

    /// Returns the [`AddressableEntityHash`] of the mint, panics if it can't be found.
    pub fn get_mint_contract_hash(&self) -> AddressableEntityHash {
        self.get_system_entity_hash(MINT)
            .expect("Unable to obtain mint contract. Please run genesis first.")
    }

    /// Returns the [`AddressableEntityHash`] of the "handle payment" contract, panics if it can't
    /// be found.
    pub fn get_handle_payment_contract_hash(&self) -> AddressableEntityHash {
        self.get_system_entity_hash(HANDLE_PAYMENT)
            .expect("Unable to obtain handle payment contract. Please run genesis first.")
    }

    /// Returns the [`AddressableEntityHash`] of the "standard payment" contract, panics if it can't
    /// be found.
    pub fn get_standard_payment_contract_hash(&self) -> AddressableEntityHash {
        self.get_system_entity_hash(STANDARD_PAYMENT)
            .expect("Unable to obtain standard payment contract. Please run genesis first.")
    }

    fn get_system_entity_hash(&self, contract_name: &str) -> Option<AddressableEntityHash> {
        self.query_system_contract_registry(self.post_state_hash)?
            .get(contract_name)
            .copied()
    }

    /// Returns the [`AddressableEntityHash`] of the "auction" contract, panics if it can't be
    /// found.
    pub fn get_auction_contract_hash(&self) -> AddressableEntityHash {
        self.get_system_entity_hash(AUCTION)
            .expect("Unable to obtain auction contract. Please run genesis first.")
    }

    /// Returns genesis effects, panics if there aren't any.
    pub fn get_genesis_effects(&self) -> &Effects {
        self.genesis_effects
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
    pub fn get_last_exec_result(&self) -> Option<Vec<Rc<ExecutionResult>>> {
        let exec_results = self.exec_results.last()?;

        Some(exec_results.iter().map(Rc::clone).collect())
    }

    /// Returns the owned results of a specific exec.
    pub fn get_exec_result_owned(&self, index: usize) -> Option<Vec<Rc<ExecutionResult>>> {
        let exec_results = self.exec_results.get(index)?;

        Some(exec_results.iter().map(Rc::clone).collect())
    }

    /// Returns a count of exec results.
    pub fn get_exec_results_count(&self) -> usize {
        self.exec_results.len()
    }

    /// Returns a `Result` containing an [`ProtocolUpgradeResult`].
    pub fn get_upgrade_result(&self, index: usize) -> Option<&ProtocolUpgradeResult> {
        self.upgrade_results.get(index)
    }

    /// Expects upgrade success.
    pub fn expect_upgrade_success(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let result = self
            .upgrade_results
            .last()
            .expect("Expected to be called after a system upgrade.");

        assert!(result.is_success(), "Expected success, got: {:?}", result);

        self
    }

    /// Returns the "handle payment" contract, panics if it can't be found.
    pub fn get_handle_payment_contract(&self) -> EntityWithNamedKeys {
        let hash = self
            .get_system_entity_hash(HANDLE_PAYMENT)
            .expect("should have handle payment contract uref");

        let handle_payment_contract = Key::addressable_entity_key(EntityKindTag::System, hash);
        let handle_payment = self
            .query(None, handle_payment_contract, &[])
            .and_then(|v| v.try_into().map_err(|error| format!("{:?}", error)))
            .expect("should find handle payment URef");

        let named_keys = self.get_named_keys(EntityAddr::System(hash.value()));
        EntityWithNamedKeys::new(handle_payment, named_keys)
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
        let state_root_hash: Digest = self.post_state_hash.expect("should have post_state_hash");
        self.engine_state
            .get_purse_balance(state_root_hash, purse)
            .expect("should get purse balance")
    }

    /// Returns a `BalanceResult` for a purse using a `PublicKey`.
    pub fn get_public_key_balance_result(&self, public_key: PublicKey) -> BalanceResult {
        let state_root_hash: Digest = self.post_state_hash.expect("should have post_state_hash");
        self.engine_state
            .get_balance(state_root_hash, public_key)
            .expect("should get purse balance using public key")
    }

    /// Gets the purse balance of a proposer.
    pub fn get_proposer_purse_balance(&self) -> U512 {
        let proposer_contract = self
            .get_entity_by_account_hash(*DEFAULT_PROPOSER_ADDR)
            .expect("proposer account should exist");
        self.get_purse_balance(proposer_contract.main_purse())
    }

    /// Gets the contract hash associated with a given account hash.
    pub fn get_entity_hash_by_account_hash(
        &self,
        account_hash: AccountHash,
    ) -> Option<AddressableEntityHash> {
        match self.query(None, Key::Account(account_hash), &[]).ok() {
            Some(StoredValue::CLValue(cl_value)) => {
                let entity_key = CLValue::into_t::<Key>(cl_value).expect("must have contract hash");
                entity_key.into_entity_hash()
            }
            Some(_) | None => None,
        }
    }

    /// Returns an Entity alongside its named keys queried by its account hash.
    pub fn get_entity_with_named_keys_by_account_hash(
        &self,
        account_hash: AccountHash,
    ) -> Option<EntityWithNamedKeys> {
        if let Some(entity) = self.get_entity_by_account_hash(account_hash) {
            let entity_named_keys = self.get_named_keys_by_account_hash(account_hash);
            return Some(EntityWithNamedKeys::new(entity, entity_named_keys));
        };

        None
    }

    /// Returns an Entity alongside its named keys queried by its entity hash.
    pub fn get_entity_with_named_keys_by_entity_hash(
        &self,
        entity_hash: AddressableEntityHash,
    ) -> Option<EntityWithNamedKeys> {
        match self.get_addressable_entity(entity_hash) {
            Some(entity) => {
                let named_keys = self.get_named_keys_by_contract_entity_hash(entity_hash);
                Some(EntityWithNamedKeys::new(entity, named_keys))
            }
            None => None,
        }
    }

    /// Queries for an `Account`.
    pub fn get_entity_by_account_hash(
        &self,
        account_hash: AccountHash,
    ) -> Option<AddressableEntity> {
        match self.query(None, Key::Account(account_hash), &[]).ok() {
            Some(StoredValue::CLValue(cl_value)) => {
                let contract_key =
                    CLValue::into_t::<Key>(cl_value).expect("must have contract hash");
                match self.query(None, contract_key, &[]) {
                    Ok(StoredValue::AddressableEntity(contract)) => Some(contract),
                    Ok(_) | Err(_) => None,
                }
            }
            Some(_other_variant) => None,
            None => None,
        }
    }

    /// Queries for an `AddressableEntity` and panics if it can't be found.
    pub fn get_expected_addressable_entity_by_account_hash(
        &self,
        account_hash: AccountHash,
    ) -> AddressableEntity {
        self.get_entity_by_account_hash(account_hash)
            .expect("account to exist")
    }

    /// Queries for an addressable entity by `AddressableEntityHash`.
    pub fn get_addressable_entity(
        &self,
        entity_hash: AddressableEntityHash,
    ) -> Option<AddressableEntity> {
        let entity_key = Key::addressable_entity_key(EntityKindTag::SmartContract, entity_hash);

        let value: StoredValue = match self.query(None, entity_key, &[]) {
            Ok(stored_value) => stored_value,
            Err(_) => self
                .query(
                    None,
                    Key::addressable_entity_key(EntityKindTag::System, entity_hash),
                    &[],
                )
                .expect("must have value"),
        };

        if let StoredValue::AddressableEntity(entity) = value {
            Some(entity)
        } else {
            None
        }
    }

    /// Retrieve a Contract from global state.
    pub fn get_legacy_contract(&self, contract_hash: ContractHash) -> Option<Contract> {
        let contract_value: StoredValue = self
            .query(None, contract_hash.into(), &[])
            .expect("should have contract value");

        if let StoredValue::Contract(contract) = contract_value {
            Some(contract)
        } else {
            None
        }
    }

    /// Queries for byte code by `ByteCodeAddr` and returns an `Option<ByteCode>`.
    pub fn get_byte_code(&self, byte_code_hash: ByteCodeHash) -> Option<ByteCode> {
        let byte_code_key = Key::byte_code_key(ByteCodeAddr::new_wasm_addr(byte_code_hash.value()));

        let byte_code_value: StoredValue = self
            .query(None, byte_code_key, &[])
            .expect("should have contract value");

        if let StoredValue::ByteCode(byte_code) = byte_code_value {
            Some(byte_code)
        } else {
            None
        }
    }

    /// Queries for a contract package by `PackageHash`.
    pub fn get_package(&self, package_hash: PackageHash) -> Option<Package> {
        let contract_value: StoredValue = self
            .query(None, package_hash.into(), &[])
            .expect("should have package value");

        if let StoredValue::Package(package) = contract_value {
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
            .get_exec_result_owned(index)
            .expect("should have exec response");
        utils::get_exec_costs(exec_results)
    }

    /// Returns the `Gas` cost of the last exec.
    pub fn last_exec_gas_cost(&self) -> Gas {
        let exec_results = self
            .get_last_exec_result()
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
        let response = self.get_exec_result_owned(index)?;
        Some(utils::get_error_message(response))
    }

    /// Gets [`EraValidators`].
    pub fn get_era_validators(&mut self) -> EraValidators {
        let state_hash = self.get_post_state_hash();
        let request = EraValidatorsRequest::new(state_hash, *DEFAULT_PROTOCOL_VERSION);
        let result = self.engine_state.get_era_validators(request);

        if let EraValidatorsResult::Success { era_validators } = result {
            era_validators
        } else {
            panic!("get era validators should be available");
        }
    }

    /// Gets [`ValidatorWeights`] for a given [`EraId`].
    pub fn get_validator_weights(&mut self, era_id: EraId) -> Option<ValidatorWeights> {
        let mut result = self.get_era_validators();
        result.remove(&era_id)
    }

    /// Gets [`Vec<BidKind>`].
    pub fn get_bids(&mut self) -> Vec<BidKind> {
        let get_bids_request = BidsRequest::new(self.get_post_state_hash());

        let get_bids_result = self.engine_state.get_bids(get_bids_request);

        get_bids_result.into_option().unwrap()
    }

    /// Returns named keys for an account entity by its account hash.
    pub fn get_named_keys_by_account_hash(&self, account_hash: AccountHash) -> NamedKeys {
        let entity_hash = self
            .get_entity_hash_by_account_hash(account_hash)
            .expect("must have entity hash");
        let entity_addr = EntityAddr::new_account_entity_addr(entity_hash.value());
        self.get_named_keys(entity_addr)
    }

    /// Returns named keys for an account entity by its entity hash.
    pub fn get_named_keys_by_contract_entity_hash(
        &self,
        contract_hash: AddressableEntityHash,
    ) -> NamedKeys {
        let entity_addr = EntityAddr::new_contract_entity_addr(contract_hash.value());
        self.get_named_keys(entity_addr)
    }

    /// Returns the named keys for a system contract.
    pub fn get_named_keys_for_system_contract(
        &self,
        system_entity_hash: AddressableEntityHash,
    ) -> NamedKeys {
        self.get_named_keys(EntityAddr::System(system_entity_hash.value()))
    }

    /// Get the named keys for an entity.
    pub fn get_named_keys(&self, entity_addr: EntityAddr) -> NamedKeys {
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .engine_state
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let prefix = entity_addr.named_keys_prefix().expect("must get prefix");

        let reader = tracking_copy.reader();

        let entries = reader.keys_with_prefix(&prefix).unwrap_or_default();

        let mut named_keys = NamedKeys::new();

        for entry in entries.iter() {
            let read_result = reader.read(entry);
            if let Ok(Some(StoredValue::NamedKey(named_key))) = read_result {
                let key = named_key.get_key().unwrap();
                let name = named_key.get_name().unwrap();
                named_keys.insert(name, key);
            }
        }

        named_keys
    }

    /// Gets [`UnbondingPurses`].
    pub fn get_unbonds(&mut self) -> UnbondingPurses {
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .engine_state
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let reader = tracking_copy.reader();

        let unbond_keys = reader
            .keys_with_prefix(&[KeyTag::Unbond as u8])
            .unwrap_or_default();

        let mut ret = BTreeMap::new();

        for key in unbond_keys.into_iter() {
            let read_result = reader.read(&key);
            if let (Key::Unbond(account_hash), Ok(Some(StoredValue::Unbonding(unbonding_purses)))) =
                (key, read_result)
            {
                ret.insert(account_hash, unbonding_purses);
            }
        }

        ret
    }

    /// Gets [`WithdrawPurses`].
    pub fn get_withdraw_purses(&mut self) -> WithdrawPurses {
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .engine_state
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let reader = tracking_copy.reader();

        let withdraws_keys = reader
            .keys_with_prefix(&[KeyTag::Withdraw as u8])
            .unwrap_or_default();

        let mut ret = BTreeMap::new();

        for key in withdraws_keys.into_iter() {
            let read_result = reader.read(&key);
            if let (Key::Withdraw(account_hash), Ok(Some(StoredValue::Withdraw(withdraw_purses)))) =
                (key, read_result)
            {
                ret.insert(account_hash, withdraw_purses);
            }
        }

        ret
    }

    /// Gets all `[Key::Balance]`s in global state.
    pub fn get_balance_keys(&self) -> Vec<Key> {
        self.get_keys(KeyTag::Balance).unwrap_or_default()
    }

    /// Gets all keys in global state by a prefix.
    pub fn get_keys(
        &self,
        tag: KeyTag,
    ) -> Result<Vec<Key>, casper_storage::global_state::error::Error> {
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .engine_state
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let reader = tracking_copy.reader();

        reader.keys_with_prefix(&[tag as u8])
    }

    /// Gets a stored value from a contract's named keys.
    pub fn get_value<T>(&mut self, entity_addr: EntityAddr, name: &str) -> T
    where
        T: FromBytes + CLTyped,
    {
        let named_keys = self.get_named_keys(entity_addr);

        let key = named_keys.get(name).expect("should have named key");
        let stored_value = self.query(None, *key, &[]).expect("should query");
        let cl_value = stored_value.into_cl_value().expect("should be cl value");
        let result: T = cl_value.into_t().expect("should convert");
        result
    }

    /// Gets an [`EraId`].
    pub fn get_era(&mut self) -> EraId {
        let auction_contract = self.get_auction_contract_hash();
        self.get_value(EntityAddr::System(auction_contract.value()), ERA_ID_KEY)
    }

    /// Gets the auction delay.
    pub fn get_auction_delay(&mut self) -> u64 {
        let auction_contract = self.get_auction_contract_hash();
        self.get_value(
            EntityAddr::System(auction_contract.value()),
            AUCTION_DELAY_KEY,
        )
    }

    /// Gets the unbonding delay
    pub fn get_unbonding_delay(&mut self) -> u64 {
        let auction_contract = self.get_auction_contract_hash();
        self.get_value(
            EntityAddr::System(auction_contract.value()),
            UNBONDING_DELAY_KEY,
        )
    }

    /// Gets the [`AddressableEntityHash`] of the system auction contract, panics if it can't be
    /// found.
    pub fn get_system_auction_hash(&self) -> AddressableEntityHash {
        let state_root_hash = self.get_post_state_hash();
        self.engine_state
            .get_system_auction_hash(state_root_hash)
            .expect("should have auction hash")
    }

    /// Gets the [`AddressableEntityHash`] of the system mint contract, panics if it can't be found.
    pub fn get_system_mint_hash(&self) -> AddressableEntityHash {
        let state_root_hash = self.get_post_state_hash();
        self.engine_state
            .get_system_mint_hash(state_root_hash)
            .expect("should have mint hash")
    }

    /// Gets the [`AddressableEntityHash`] of the system handle payment contract, panics if it can't
    /// be found.
    pub fn get_system_handle_payment_hash(&self) -> AddressableEntityHash {
        let state_root_hash = self.get_post_state_hash();
        self.engine_state
            .get_handle_payment_hash(state_root_hash)
            .expect("should have handle payment hash")
    }

    /// Resets the `exec_results`, `upgrade_results` and `transform` fields.
    pub fn clear_results(&mut self) -> &mut Self {
        self.exec_results = Vec::new();
        self.upgrade_results = Vec::new();
        self.effects = Vec::new();
        self
    }

    /// Advances eras by num_eras
    pub fn advance_eras_by(&mut self, num_eras: u64) {
        let step_request_builder = StepRequestBuilder::new()
            .with_protocol_version(ProtocolVersion::V1_0_0)
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
    pub fn advance_eras_by_default_auction_delay(&mut self) {
        let auction_delay = self.get_auction_delay();
        self.advance_eras_by(auction_delay + 1);
    }

    /// Advances by a single era.
    pub fn advance_era(&mut self) {
        self.advance_eras_by(1);
    }

    /// Returns a trie by hash.
    pub fn get_trie(&mut self, state_hash: Digest) -> Option<Trie<Key, StoredValue>> {
        self.engine_state
            .get_trie_full(state_hash)
            .unwrap()
            .map(|bytes| bytesrepr::deserialize(bytes.into_inner().into()).unwrap())
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

    /// Commits a prune of leaf nodes from the tip of the merkle trie.
    pub fn commit_prune(&mut self, prune_config: PruneConfig) -> &mut Self {
        let result = self.engine_state.commit_prune(prune_config);

        if let Ok(PruneResult::Success {
            post_state_hash,
            effects,
        }) = &result
        {
            self.post_state_hash = Some(*post_state_hash);
            self.effects.push(effects.clone());
        }

        self.prune_results.push(result);
        self
    }

    /// Returns a `Result` containing a [`PruneResult`].
    pub fn get_prune_result(&self, index: usize) -> Option<&Result<PruneResult, Error>> {
        self.prune_results.get(index)
    }

    /// Expects a prune success.
    pub fn expect_prune_success(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let result = self
            .prune_results
            .last()
            .expect("Expected to be called after a system upgrade.")
            .as_ref();

        let prune_result = result.unwrap_or_else(|_| panic!("Expected success, got: {:?}", result));
        match prune_result {
            PruneResult::RootNotFound => panic!("Root not found"),
            PruneResult::DoesNotExist => panic!("Does not exists"),
            PruneResult::Success { .. } => {}
        }

        self
    }

    /// Returns the results of all execs.
    #[deprecated(
        since = "2.3.0",
        note = "use `get_last_exec_results` or `get_exec_result_owned` instead"
    )]
    pub fn get_exec_results(&self) -> &Vec<Vec<Rc<ExecutionResult>>> {
        &self.exec_results
    }

    /// Returns the results of a specific exec.
    #[deprecated(since = "2.3.0", note = "use `get_exec_result_owned` instead")]
    pub fn get_exec_result(&self, index: usize) -> Option<&Vec<Rc<ExecutionResult>>> {
        self.exec_results.get(index)
    }

    /// Gets [`UnbondingPurses`].
    #[deprecated(since = "2.3.0", note = "use `get_withdraw_purses` instead")]
    pub fn get_withdraws(&mut self) -> UnbondingPurses {
        let withdraw_purses = self.get_withdraw_purses();
        let unbonding_purses: UnbondingPurses = withdraw_purses
            .iter()
            .map(|(key, withdraw_purse)| {
                (
                    key.to_owned(),
                    withdraw_purse
                        .iter()
                        .map(|withdraw_purse| withdraw_purse.to_owned().into())
                        .collect::<Vec<UnbondingPurse>>(),
                )
            })
            .collect::<BTreeMap<AccountHash, Vec<UnbondingPurse>>>();
        unbonding_purses
    }

    /// Calculates refunded amount from a last execution request.
    pub fn calculate_refund_amount(&self, payment_amount: U512) -> U512 {
        let gas_amount = Motes::from_gas(self.last_exec_gas_cost(), DEFAULT_GAS_PRICE)
            .expect("should create motes from gas");

        let refund_ratio = match self.engine_state.config().refund_handling() {
            RefundHandling::Refund { refund_ratio } | RefundHandling::Burn { refund_ratio } => {
                *refund_ratio
            }
        };

        let (numer, denom) = refund_ratio.into();
        let refund_ratio = Ratio::new_raw(U512::from(numer), U512::from(denom));

        // amount declared to be paid in payment code MINUS gas spent in last execution.
        let refundable_amount = Ratio::from(payment_amount) - Ratio::from(gas_amount.value());
        (refundable_amount * refund_ratio).to_integer()
    }
}
