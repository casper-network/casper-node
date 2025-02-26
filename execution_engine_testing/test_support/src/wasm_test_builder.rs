use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    ffi::OsStr,
    fs,
    iter::{self, FromIterator},
    ops::Deref,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use filesize::PathExt;
use lmdb::DatabaseFlags;
use num_rational::Ratio;
use num_traits::{CheckedMul, Zero};
use tempfile::TempDir;

use casper_execution_engine::engine_state::{
    EngineConfig, Error, ExecutionEngineV1, WasmV1Request, WasmV1Result, DEFAULT_MAX_QUERY_DEPTH,
};
use casper_storage::{
    data_access_layer::{
        balance::BalanceHandling,
        forced_undelegate::{ForcedUndelegateRequest, ForcedUndelegateResult},
        AuctionMethod, BalanceIdentifier, BalanceRequest, BalanceResult, BiddingRequest,
        BiddingResult, BidsRequest, BlockRewardsRequest, BlockRewardsResult, BlockStore,
        DataAccessLayer, EraValidatorsRequest, EraValidatorsResult, FeeRequest, FeeResult,
        FlushRequest, FlushResult, GenesisRequest, GenesisResult, HandleFeeMode, HandleFeeRequest,
        HandleFeeResult, MessageTopicsRequest, MessageTopicsResult, ProofHandling,
        ProtocolUpgradeRequest, ProtocolUpgradeResult, PruneRequest, PruneResult, QueryRequest,
        QueryResult, RoundSeigniorageRateRequest, RoundSeigniorageRateResult, StepRequest,
        StepResult, SystemEntityRegistryPayload, SystemEntityRegistryRequest,
        SystemEntityRegistryResult, SystemEntityRegistrySelector, TotalSupplyRequest,
        TotalSupplyResult, TransferRequest, TrieRequest,
    },
    global_state::{
        state::{
            lmdb::LmdbGlobalState, scratch::ScratchGlobalState, CommitProvider, ScratchProvider,
            StateProvider, StateReader,
        },
        transaction_source::lmdb::LmdbEnvironment,
        trie::Trie,
        trie_store::lmdb::LmdbTrieStore,
    },
    system::runtime_native::{Config as NativeRuntimeConfig, TransferConfig},
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyExt},
    AddressGenerator,
};

use casper_types::{
    account::AccountHash,
    addressable_entity::{EntityKindTag, MessageTopics, NamedKeyAddr},
    bytesrepr::{self, FromBytes},
    contracts::{ContractHash, NamedKeys},
    execution::Effects,
    global_state::TrieMerkleProof,
    runtime_args,
    system::{
        auction::{
            BidKind, EraValidators, Unbond, UnbondKind, UnbondingPurse, ValidatorWeights,
            WithdrawPurses, ARG_ERA_END_TIMESTAMP_MILLIS, ARG_EVICTED_VALIDATORS,
            AUCTION_DELAY_KEY, ERA_ID_KEY, METHOD_RUN_AUCTION, UNBONDING_DELAY_KEY,
        },
        mint::{MINT_GAS_HOLD_HANDLING_KEY, MINT_GAS_HOLD_INTERVAL_KEY},
        AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
    },
    AccessRights, Account, AddressableEntity, AddressableEntityHash, AuctionCosts, BlockGlobalAddr,
    BlockTime, ByteCode, ByteCodeAddr, ByteCodeHash, CLTyped, CLValue, Contract, Digest,
    EntityAddr, EntryPoints, EraId, FeeHandling, Gas, HandlePaymentCosts, HashAddr,
    HoldBalanceHandling, InitiatorAddr, Key, KeyTag, MintCosts, Motes, Package, PackageHash, Phase,
    ProtocolUpgradeConfig, ProtocolVersion, PublicKey, RefundHandling, StoredValue,
    SystemHashRegistry, TransactionHash, TransactionV1Hash, URef, OS_PAGE_SIZE, U512,
};

use crate::{
    chainspec_config::{ChainspecConfig, CHAINSPEC_SYMLINK},
    ExecuteRequest, ExecuteRequestBuilder, StepRequestBuilder, DEFAULT_GAS_PRICE,
    DEFAULT_PROPOSER_ADDR, DEFAULT_PROTOCOL_VERSION, SYSTEM_ADDR,
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
#[derive(Debug)]
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

/// Builder for simple WASM test
pub struct WasmTestBuilder<S> {
    /// Data access layer.
    data_access_layer: Arc<S>,
    /// [`ExecutionEngineV1`] is wrapped in [`Rc`] to work around a missing [`Clone`]
    /// implementation.
    execution_engine: Rc<ExecutionEngineV1>,
    /// The chainspec.
    chainspec: ChainspecConfig,
    exec_results: Vec<WasmV1Result>,
    upgrade_results: Vec<ProtocolUpgradeResult>,
    prune_results: Vec<PruneResult>,
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
    scratch_global_state: Option<ScratchGlobalState>,
    /// Global state dir, for implementations that define one.
    global_state_dir: Option<PathBuf>,
    /// Temporary directory, for implementation that uses one.
    temp_dir: Option<Rc<TempDir>>,
}

impl<S: ScratchProvider> WasmTestBuilder<S> {
    /// Commit scratch to global state, and reset the scratch cache.
    pub fn write_scratch_to_db(&mut self) -> &mut Self {
        let prestate_hash = self.post_state_hash.expect("Should have genesis hash");
        if let Some(scratch) = self.scratch_global_state.take() {
            let new_state_root = self
                .data_access_layer
                .write_scratch_to_db(prestate_hash, scratch)
                .unwrap();
            self.post_state_hash = Some(new_state_root);
        }
        self
    }
    /// Flushes the LMDB environment to disk.
    pub fn flush_environment(&self) {
        let request = FlushRequest::new();
        if let FlushResult::Failure(gse) = self.data_access_layer.flush(request) {
            panic!("flush failed: {:?}", gse)
        }
    }

    /// Execute and commit transforms from an ExecuteRequest into a scratch global state.
    /// You MUST call write_scratch_to_lmdb to flush these changes to LmdbGlobalState.
    #[allow(deprecated)]
    pub fn scratch_exec_and_commit(&mut self, mut exec_request: WasmV1Request) -> &mut Self {
        if self.scratch_global_state.is_none() {
            self.scratch_global_state = Some(self.data_access_layer.get_scratch_global_state());
        }

        let cached_state = self
            .scratch_global_state
            .as_ref()
            .expect("scratch state should exist");

        let state_hash = self.post_state_hash.expect("expected post_state_hash");
        exec_request.block_info.with_state_hash(state_hash);

        // First execute the request against our scratch global state.
        let execution_result = self.execution_engine.execute(cached_state, exec_request);
        let _post_state_hash = cached_state
            .commit_effects(
                self.post_state_hash.expect("requires a post_state_hash"),
                execution_result.effects().clone(),
            )
            .expect("should commit");

        // Save transforms and execution results for WasmTestBuilder.
        self.effects.push(execution_result.effects().clone());
        self.exec_results.push(execution_result);
        self
    }
}

impl<S> Clone for WasmTestBuilder<S> {
    fn clone(&self) -> Self {
        WasmTestBuilder {
            data_access_layer: Arc::clone(&self.data_access_layer),
            execution_engine: Rc::clone(&self.execution_engine),
            chainspec: self.chainspec.clone(),
            exec_results: self.exec_results.clone(),
            upgrade_results: self.upgrade_results.clone(),
            prune_results: self.prune_results.clone(),
            genesis_hash: self.genesis_hash,
            post_state_hash: self.post_state_hash,
            effects: self.effects.clone(),
            genesis_effects: self.genesis_effects.clone(),
            system_account: self.system_account.clone(),
            scratch_global_state: None,
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

/// Wasm test builder where state is held in LMDB.
pub type LmdbWasmTestBuilder = WasmTestBuilder<DataAccessLayer<LmdbGlobalState>>;

impl Default for LmdbWasmTestBuilder {
    fn default() -> Self {
        Self::new_temporary_with_chainspec(&*CHAINSPEC_SYMLINK)
    }
}

impl LmdbWasmTestBuilder {
    /// Upgrades the execution engine using the scratch trie.
    pub fn upgrade_using_scratch(
        &mut self,
        upgrade_config: &mut ProtocolUpgradeConfig,
    ) -> &mut Self {
        let pre_state_hash = self.post_state_hash.expect("should have state hash");
        upgrade_config.with_pre_state_hash(pre_state_hash);

        let scratch_state = self.data_access_layer.get_scratch_global_state();
        let pre_state_hash = upgrade_config.pre_state_hash();
        let req = ProtocolUpgradeRequest::new(upgrade_config.clone());
        let result = {
            let result = scratch_state.protocol_upgrade(req);
            if let ProtocolUpgradeResult::Success { effects, .. } = result {
                let post_state_hash = self
                    .data_access_layer
                    .write_scratch_to_db(pre_state_hash, scratch_state)
                    .unwrap();
                self.post_state_hash = Some(post_state_hash);
                let mut engine_config = self.chainspec.engine_config();
                engine_config.set_protocol_version(upgrade_config.new_protocol_version());
                self.execution_engine = Rc::new(ExecutionEngineV1::new(engine_config));
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
        chainspec: ChainspecConfig,
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
        let enable_addressable_entity = chainspec.core_config.enable_addressable_entity;
        let global_state = LmdbGlobalState::empty(
            environment,
            trie_store,
            max_query_depth,
            enable_addressable_entity,
        )
        .expect("should create LmdbGlobalState");

        let data_access_layer = Arc::new(DataAccessLayer {
            block_store: BlockStore::new(),
            state: global_state,
            max_query_depth,
            enable_addressable_entity,
        });

        let engine_config = chainspec.engine_config();
        let engine_state = ExecutionEngineV1::new(engine_config);

        WasmTestBuilder {
            data_access_layer,
            execution_engine: Rc::new(engine_state),
            chainspec,
            exec_results: Vec::new(),
            upgrade_results: Vec::new(),
            prune_results: Vec::new(),
            genesis_hash: None,
            post_state_hash: None,
            effects: Vec::new(),
            system_account: None,
            genesis_effects: None,
            scratch_global_state: None,
            global_state_dir: Some(global_state_dir),
            temp_dir: None,
        }
    }

    fn create_or_open<T: AsRef<Path>>(
        global_state_dir: T,
        chainspec: ChainspecConfig,
        protocol_version: ProtocolVersion,
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

        let enable_addressable_entity = chainspec.core_config.enable_addressable_entity;
        let global_state = match mode {
            GlobalStateMode::Create(database_flags) => {
                let trie_store = LmdbTrieStore::new(&environment, None, database_flags)
                    .expect("should open LmdbTrieStore");
                LmdbGlobalState::empty(
                    Arc::new(environment),
                    Arc::new(trie_store),
                    max_query_depth,
                    enable_addressable_entity,
                )
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
                    enable_addressable_entity,
                )
            }
        };

        let data_access_layer = Arc::new(DataAccessLayer {
            block_store: BlockStore::new(),
            state: global_state,
            max_query_depth,
            enable_addressable_entity,
        });
        let mut engine_config = chainspec.engine_config();
        engine_config.set_protocol_version(protocol_version);
        let engine_state = ExecutionEngineV1::new(engine_config);

        let post_state_hash = mode.post_state_hash();

        let builder = WasmTestBuilder {
            data_access_layer,
            execution_engine: Rc::new(engine_state),
            chainspec,
            exec_results: Vec::new(),
            upgrade_results: Vec::new(),
            prune_results: Vec::new(),
            genesis_hash: None,
            post_state_hash,
            effects: Vec::new(),
            genesis_effects: None,
            system_account: None,
            scratch_global_state: None,
            global_state_dir: Some(global_state_dir.as_ref().to_path_buf()),
            temp_dir: None,
        };

        builder
    }

    /// Returns an [`LmdbWasmTestBuilder`] with configuration and values from
    /// a given chainspec.
    pub fn new_with_chainspec<T: AsRef<OsStr> + ?Sized, P: AsRef<Path>>(
        data_dir: &T,
        chainspec_path: P,
    ) -> Self {
        let chainspec_config = ChainspecConfig::from_chainspec_path(chainspec_path)
            .expect("must build chainspec configuration");

        Self::new_with_config(data_dir, chainspec_config)
    }

    /// Returns an [`LmdbWasmTestBuilder`] with configuration and values from
    /// the production chainspec.
    pub fn new_with_production_chainspec<T: AsRef<OsStr> + ?Sized>(data_dir: &T) -> Self {
        Self::new_with_chainspec(data_dir, &*CHAINSPEC_SYMLINK)
    }

    /// Returns a new [`LmdbWasmTestBuilder`].
    pub fn new<T: AsRef<OsStr> + ?Sized>(data_dir: &T) -> Self {
        Self::new_with_config(data_dir, Default::default())
    }

    /// Creates a new instance of builder using the supplied configurations, opening wrapped LMDBs
    /// (e.g. in the Trie and Data stores) rather than creating them.
    pub fn open<T: AsRef<OsStr> + ?Sized>(
        data_dir: &T,
        chainspec: ChainspecConfig,
        protocol_version: ProtocolVersion,
        post_state_hash: Digest,
    ) -> Self {
        let global_state_path = Self::global_state_dir(data_dir);
        Self::open_raw(
            global_state_path,
            chainspec,
            protocol_version,
            post_state_hash,
        )
    }

    /// Creates a new instance of builder using the supplied configurations, opening wrapped LMDBs
    /// (e.g. in the Trie and Data stores) rather than creating them.
    /// Differs from `open` in that it doesn't append `GLOBAL_STATE_DIR` to the supplied path.
    pub fn open_raw<T: AsRef<Path>>(
        global_state_dir: T,
        chainspec: ChainspecConfig,
        protocol_version: ProtocolVersion,
        post_state_hash: Digest,
    ) -> Self {
        Self::create_or_open(
            global_state_dir,
            chainspec,
            protocol_version,
            GlobalStateMode::Open(post_state_hash),
        )
    }

    /// Creates new temporary lmdb builder with an engine config instance.
    ///
    /// Once [`LmdbWasmTestBuilder`] instance goes out of scope a global state directory will be
    /// removed as well.
    pub fn new_temporary_with_config(chainspec: ChainspecConfig) -> Self {
        let temp_dir = tempfile::tempdir().unwrap();

        let database_flags = DatabaseFlags::default();

        let mut builder = Self::create_or_open(
            temp_dir.path(),
            chainspec,
            DEFAULT_PROTOCOL_VERSION,
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
        let chainspec = ChainspecConfig::from_chainspec_path(chainspec_path)
            .expect("must build chainspec configuration");

        Self::new_temporary_with_config(chainspec)
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

    /// run step against scratch global state.
    pub fn step_with_scratch(&mut self, step_request: StepRequest) -> &mut Self {
        if self.scratch_global_state.is_none() {
            self.scratch_global_state = Some(self.data_access_layer.get_scratch_global_state());
        }

        let cached_state = self
            .scratch_global_state
            .as_ref()
            .expect("scratch state should exist");

        match cached_state.step(step_request) {
            StepResult::RootNotFound => {
                panic!("Root not found")
            }
            StepResult::Failure(err) => {
                panic!("{:?}", err)
            }
            StepResult::Success { .. } => {}
        }
        self
    }

    /// Runs a [`TransferRequest`] and commits the resulting effects.
    pub fn transfer_and_commit(&mut self, mut transfer_request: TransferRequest) -> &mut Self {
        let pre_state_hash = self.post_state_hash.expect("expected post_state_hash");
        transfer_request.set_state_hash_and_config(pre_state_hash, self.native_runtime_config());
        let transfer_result = self.data_access_layer.transfer(transfer_request);
        let gas = Gas::new(self.chainspec.system_costs_config.mint_costs().transfer);
        let execution_result = WasmV1Result::from_transfer_result(transfer_result, gas)
            .expect("transfer result should map to wasm v1 result");
        let effects = execution_result.effects().clone();
        self.effects.push(effects.clone());
        self.exec_results.push(execution_result);
        self.commit_transforms(pre_state_hash, effects);
        self
    }
}

impl<S> WasmTestBuilder<S>
where
    S: StateProvider + CommitProvider,
{
    /// Takes a [`GenesisRequest`], executes the request and returns Self.
    pub fn run_genesis(&mut self, request: GenesisRequest) -> &mut Self {
        match self.data_access_layer.genesis(request) {
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

    fn query_system_entity_registry(
        &self,
        post_state_hash: Option<Digest>,
    ) -> Option<SystemHashRegistry> {
        match self.query(post_state_hash, Key::SystemEntityRegistry, &[]) {
            Ok(StoredValue::CLValue(cl_registry)) => {
                let system_entity_registry =
                    CLValue::into_t::<SystemHashRegistry>(cl_registry).unwrap();
                Some(system_entity_registry)
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

        let query_result = self.data_access_layer.query(query_request);
        if let QueryResult::Success { value, .. } = query_result {
            return Ok(value.deref().clone());
        }

        Err(format!("{:?}", query_result))
    }

    /// Retrieves the message topics for the given hash addr.
    pub fn message_topics(
        &self,
        maybe_post_state: Option<Digest>,
        hash_addr: HashAddr,
    ) -> Result<MessageTopics, String> {
        let post_state = maybe_post_state
            .or(self.post_state_hash)
            .expect("builder must have a post-state hash");

        let request = MessageTopicsRequest::new(post_state, hash_addr);
        let result = self.data_access_layer.message_topics(request);
        if let MessageTopicsResult::Success { message_topics } = result {
            return Ok(message_topics);
        }

        Err(format!("{:?}", result))
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
            .map(|entity_hash| EntityAddr::new_account(entity_hash.value()))
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

        let query_result = self.data_access_layer.query(query_request);

        if let QueryResult::Success { value, proofs } = query_result {
            return Ok((value.deref().clone(), proofs));
        }

        panic! {"{:?}", query_result};
    }

    /// Queries for the total supply of token.
    /// # Panics
    /// Panics if the total supply can't be found.
    pub fn total_supply(
        &self,
        protocol_version: ProtocolVersion,
        maybe_post_state: Option<Digest>,
    ) -> U512 {
        let post_state = maybe_post_state
            .or(self.post_state_hash)
            .expect("builder must have a post-state hash");
        let result = self
            .data_access_layer
            .total_supply(TotalSupplyRequest::new(post_state, protocol_version));
        if let TotalSupplyResult::Success { total_supply } = result {
            total_supply
        } else {
            panic!("total supply should exist at every root hash {:?}", result);
        }
    }

    /// Queries for the round seigniorage rate.
    /// # Panics
    /// Panics if the total supply or seigniorage rate can't be found.
    pub fn round_seigniorage_rate(
        &mut self,
        maybe_post_state: Option<Digest>,
        protocol_version: ProtocolVersion,
    ) -> Ratio<U512> {
        let post_state = maybe_post_state
            .or(self.post_state_hash)
            .expect("builder must have a post-state hash");
        let result =
            self.data_access_layer
                .round_seigniorage_rate(RoundSeigniorageRateRequest::new(
                    post_state,
                    protocol_version,
                ));
        if let RoundSeigniorageRateResult::Success { rate } = result {
            rate
        } else {
            panic!(
                "round seigniorage rate should exist at every root hash {:?}",
                result
            );
        }
    }

    /// Queries for the base round reward.
    /// # Panics
    /// Panics if the total supply or seigniorage rate can't be found.
    pub fn base_round_reward(
        &mut self,
        maybe_post_state: Option<Digest>,
        protocol_version: ProtocolVersion,
    ) -> U512 {
        let post_state = maybe_post_state
            .or(self.post_state_hash)
            .expect("builder must have a post-state hash");
        let total_supply = self.total_supply(protocol_version, Some(post_state));
        let rate = self.round_seigniorage_rate(Some(post_state), protocol_version);
        rate.checked_mul(&Ratio::from(total_supply))
            .map(|ratio| ratio.to_integer())
            .expect("must get base round reward")
    }

    /// Direct auction interactions for stake management.
    pub fn bidding(
        &mut self,
        maybe_post_state: Option<Digest>,
        protocol_version: ProtocolVersion,
        initiator: InitiatorAddr,
        auction_method: AuctionMethod,
    ) -> BiddingResult {
        let post_state = maybe_post_state
            .or(self.post_state_hash)
            .expect("builder must have a post-state hash");

        let transaction_hash = TransactionHash::V1(TransactionV1Hash::default());
        let authorization_keys = BTreeSet::from_iter(iter::once(initiator.account_hash()));

        let config = &self.chainspec;
        let fee_handling = config.core_config.fee_handling;
        let refund_handling = config.core_config.refund_handling;
        let vesting_schedule_period_millis = config.core_config.vesting_schedule_period.millis();
        let allow_auction_bids = config.core_config.allow_auction_bids;
        let compute_rewards = config.core_config.compute_rewards;
        let max_delegators_per_validator = config.core_config.max_delegators_per_validator;
        let minimum_delegation_amount = config.core_config.minimum_delegation_amount;
        let balance_hold_interval = config.core_config.gas_hold_interval.millis();
        let include_credits = config.core_config.fee_handling == FeeHandling::NoFee;
        let credit_cap = Ratio::new_raw(
            U512::from(*config.core_config.validator_credit_cap.numer()),
            U512::from(*config.core_config.validator_credit_cap.denom()),
        );
        let enable_addressable_entity = config.core_config.enable_addressable_entity;
        let native_runtime_config = casper_storage::system::runtime_native::Config::new(
            TransferConfig::Unadministered,
            fee_handling,
            refund_handling,
            vesting_schedule_period_millis,
            allow_auction_bids,
            compute_rewards,
            max_delegators_per_validator,
            minimum_delegation_amount,
            balance_hold_interval,
            include_credits,
            credit_cap,
            enable_addressable_entity,
            config.system_costs_config.mint_costs().transfer,
        );

        let bidding_req = BiddingRequest::new(
            native_runtime_config,
            post_state,
            protocol_version,
            transaction_hash,
            initiator,
            authorization_keys,
            auction_method,
        );
        self.data_access_layer().bidding(bidding_req)
    }

    /// Runs an optional custom payment [`WasmV1Request`] and a session `WasmV1Request`.
    ///
    /// If the custom payment is `Some` and its execution fails, the session request is not
    /// attempted.
    pub fn exec_wasm_v1(&mut self, mut request: WasmV1Request) -> &mut Self {
        let state_hash = self.post_state_hash.expect("expected post_state_hash");
        request.block_info.with_state_hash(state_hash);
        let result = self
            .execution_engine
            .execute(self.data_access_layer.as_ref(), request);
        let effects = result.effects().clone();
        self.exec_results.push(result);
        self.effects.push(effects);
        self
    }

    /// Runs an [`ExecuteRequest`].
    pub fn exec(&mut self, mut exec_request: ExecuteRequest) -> &mut Self {
        let mut effects = Effects::new();
        if let Some(mut payment) = exec_request.custom_payment {
            let state_hash = self.post_state_hash.expect("expected post_state_hash");
            payment.block_info.with_state_hash(state_hash);
            let payment_result = self
                .execution_engine
                .execute(self.data_access_layer.as_ref(), payment);
            // If executing payment code failed, record this and exit without attempting session
            // execution.
            effects = payment_result.effects().clone();
            let payment_failed = payment_result.error().is_some();
            self.exec_results.push(payment_result);
            if payment_failed {
                self.effects.push(effects);
                return self;
            }
        }
        let state_hash = self.post_state_hash.expect("expected post_state_hash");
        exec_request.session.block_info.with_state_hash(state_hash);

        let session_result = self
            .execution_engine
            .execute(self.data_access_layer.as_ref(), exec_request.session);
        // Cache transformations
        effects.append(session_result.effects().clone());
        self.effects.push(effects);
        self.exec_results.push(session_result);
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
            .data_access_layer
            .commit_effects(pre_state_hash, effects)
            .expect("should commit");
        self.post_state_hash = Some(post_state_hash);
        self
    }

    /// Upgrades the execution engine.
    pub fn upgrade(&mut self, upgrade_config: &mut ProtocolUpgradeConfig) -> &mut Self {
        let pre_state_hash = self.post_state_hash.expect("should have state hash");
        upgrade_config.with_pre_state_hash(pre_state_hash);

        let req = ProtocolUpgradeRequest::new(upgrade_config.clone());

        let result = self.data_access_layer.protocol_upgrade(req);

        if let ProtocolUpgradeResult::Success {
            post_state_hash, ..
        } = result
        {
            let mut engine_config = self.chainspec.engine_config();
            engine_config.set_protocol_version(upgrade_config.new_protocol_version());
            self.execution_engine = Rc::new(ExecutionEngineV1::new(engine_config));
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
        let exec_request = ExecuteRequestBuilder::contract_call_by_hash(
            *SYSTEM_ADDR,
            auction,
            METHOD_RUN_AUCTION,
            runtime_args! {
                ARG_ERA_END_TIMESTAMP_MILLIS => era_end_timestamp_millis,
                ARG_EVICTED_VALIDATORS => evicted_validators,
            },
        )
        .build();
        self.exec(exec_request).expect_success().commit()
    }

    /// Increments engine state.
    pub fn step(&mut self, step_request: StepRequest) -> StepResult {
        let step_result = self.data_access_layer.step(step_request);

        if let StepResult::Success {
            post_state_hash, ..
        } = step_result
        {
            self.post_state_hash = Some(post_state_hash);
        }

        step_result
    }

    fn native_runtime_config(&self) -> NativeRuntimeConfig {
        let administrators: BTreeSet<AccountHash> = self
            .chainspec
            .core_config
            .administrators
            .iter()
            .map(|x| x.to_account_hash())
            .collect();
        let allow_unrestricted = self.chainspec.core_config.allow_unrestricted_transfers;
        let transfer_config = TransferConfig::new(administrators, allow_unrestricted);
        let include_credits = self.chainspec.core_config.fee_handling == FeeHandling::NoFee;
        let credit_cap = Ratio::new_raw(
            U512::from(*self.chainspec.core_config.validator_credit_cap.numer()),
            U512::from(*self.chainspec.core_config.validator_credit_cap.denom()),
        );
        NativeRuntimeConfig::new(
            transfer_config,
            self.chainspec.core_config.fee_handling,
            self.chainspec.core_config.refund_handling,
            self.chainspec.core_config.vesting_schedule_period.millis(),
            self.chainspec.core_config.allow_auction_bids,
            self.chainspec.core_config.compute_rewards,
            self.chainspec.core_config.max_delegators_per_validator,
            self.chainspec.core_config.minimum_delegation_amount,
            self.chainspec.core_config.gas_hold_interval.millis(),
            include_credits,
            credit_cap,
            self.chainspec.core_config.enable_addressable_entity,
            self.chainspec.system_costs_config.mint_costs().transfer,
        )
    }

    /// Distribute fees.
    pub fn distribute_fees(
        &mut self,
        pre_state_hash: Option<Digest>,
        protocol_version: ProtocolVersion,
        block_time: u64,
    ) -> FeeResult {
        let native_runtime_config = self.native_runtime_config();

        let pre_state_hash = pre_state_hash.or(self.post_state_hash).unwrap();
        let fee_req = FeeRequest::new(
            native_runtime_config,
            pre_state_hash,
            protocol_version,
            block_time.into(),
        );
        let fee_result = self.data_access_layer.distribute_fees(fee_req);

        if let FeeResult::Success {
            post_state_hash, ..
        } = fee_result
        {
            self.post_state_hash = Some(post_state_hash);
        }

        fee_result
    }

    /// Distributes the rewards.
    pub fn distribute(
        &mut self,
        pre_state_hash: Option<Digest>,
        protocol_version: ProtocolVersion,
        rewards: BTreeMap<PublicKey, Vec<U512>>,
        block_time: u64,
    ) -> BlockRewardsResult {
        let pre_state_hash = pre_state_hash.or(self.post_state_hash).unwrap();
        let native_runtime_config = self.native_runtime_config();
        let distribute_req = BlockRewardsRequest::new(
            native_runtime_config,
            pre_state_hash,
            protocol_version,
            BlockTime::new(block_time),
            rewards,
        );
        let distribute_block_rewards_result = self
            .data_access_layer
            .distribute_block_rewards(distribute_req);

        if let BlockRewardsResult::Success {
            post_state_hash, ..
        } = distribute_block_rewards_result
        {
            self.post_state_hash = Some(post_state_hash);
        }

        distribute_block_rewards_result
    }

    /// Undelegates delegator bids violating configured delegation limits.
    pub fn forced_undelegate(
        &mut self,
        pre_state_hash: Option<Digest>,
        protocol_version: ProtocolVersion,
        block_time: u64,
    ) -> ForcedUndelegateResult {
        let pre_state_hash = pre_state_hash.or(self.post_state_hash).unwrap();
        let native_runtime_config = self.native_runtime_config();
        let forced_undelegate_req = ForcedUndelegateRequest::new(
            native_runtime_config,
            pre_state_hash,
            protocol_version,
            BlockTime::new(block_time),
        );
        let forced_undelegate_result = self
            .data_access_layer
            .forced_undelegate(forced_undelegate_req);

        if let ForcedUndelegateResult::Success {
            post_state_hash, ..
        } = forced_undelegate_result
        {
            self.post_state_hash = Some(post_state_hash);
        }

        forced_undelegate_result
    }

    /// Finalizes payment for a transaction
    pub fn handle_fee(
        &mut self,
        pre_state_hash: Option<Digest>,
        protocol_version: ProtocolVersion,
        transaction_hash: TransactionHash,
        handle_fee_mode: HandleFeeMode,
    ) -> HandleFeeResult {
        let pre_state_hash = pre_state_hash.or(self.post_state_hash).unwrap();
        let native_runtime_config = self.native_runtime_config();
        let handle_fee_request = HandleFeeRequest::new(
            native_runtime_config,
            pre_state_hash,
            protocol_version,
            transaction_hash,
            handle_fee_mode,
        );
        let handle_fee_result = self.data_access_layer.handle_fee(handle_fee_request);
        if let HandleFeeResult::Success { effects, .. } = &handle_fee_result {
            self.commit_transforms(pre_state_hash, effects.clone());
        }

        handle_fee_result
    }

    /// Expects a successful run
    #[track_caller]
    pub fn expect_success(&mut self) -> &mut Self {
        let exec_result = self
            .get_last_exec_result()
            .expect("Expected to be called after exec()");
        if exec_result.error().is_some() {
            panic!(
                "Expected successful execution result, but instead got: {:#?}",
                exec_result,
            );
        }
        self
    }

    /// Expects a failed run
    pub fn expect_failure(&mut self) -> &mut Self {
        let exec_result = self
            .get_last_exec_result()
            .expect("Expected to be called after exec()");
        if exec_result.error().is_none() {
            panic!(
                "Expected failed execution result, but instead got: {:?}",
                exec_result,
            );
        }
        self
    }

    /// Returns `true` if the last exec had an error, otherwise returns false.
    #[track_caller]
    pub fn is_error(&self) -> bool {
        self.get_last_exec_result()
            .expect("Expected to be called after exec()")
            .error()
            .is_some()
    }

    /// Returns an `engine_state::Error` if the last exec had an error, otherwise `None`.
    #[track_caller]
    pub fn get_error(&self) -> Option<Error> {
        self.get_last_exec_result()
            .expect("Expected to be called after exec()")
            .error()
            .cloned()
    }

    /// Returns the error message of the last exec.
    #[track_caller]
    pub fn get_error_message(&self) -> Option<String> {
        self.get_last_exec_result()
            .expect("Expected to be called after exec()")
            .error()
            .map(|error| error.to_string())
    }

    /// Gets `Effects` of all previous runs.
    #[track_caller]
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
        self.query_system_entity_registry(self.post_state_hash)?
            .get(contract_name)
            .map(|hash| AddressableEntityHash::new(*hash))
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

    /// The chainspec configured settings for this builder.
    pub fn chainspec(&self) -> &ChainspecConfig {
        &self.chainspec
    }

    /// Update chainspec
    pub fn with_chainspec(&mut self, chainspec: ChainspecConfig) -> &mut Self {
        self.chainspec = chainspec;
        self.execution_engine = Rc::new(ExecutionEngineV1::new(self.chainspec.engine_config()));
        self
    }

    /// Update the engine config of the builder.
    pub fn with_engine_config(&mut self, engine_config: EngineConfig) -> &mut Self {
        self.execution_engine = Rc::new(ExecutionEngineV1::new(engine_config));
        self
    }

    /// Sets blocktime into global state.
    pub fn with_block_time(&mut self, block_time: BlockTime) -> &mut Self {
        if let Some(state_root_hash) = self.post_state_hash {
            let mut tracking_copy = self
                .data_access_layer
                .tracking_copy(state_root_hash)
                .expect("should not error on checkout")
                .expect("should checkout tracking copy");

            let cl_value = CLValue::from_t(block_time.value()).expect("should get cl value");
            tracking_copy.write(
                Key::BlockGlobal(BlockGlobalAddr::BlockTime),
                StoredValue::CLValue(cl_value),
            );
            self.commit_transforms(state_root_hash, tracking_copy.effects());
        }

        self
    }

    /// Writes a set of keys and values to global state.
    pub fn write_data_and_commit(
        &mut self,
        data: impl Iterator<Item = (Key, StoredValue)>,
    ) -> &mut Self {
        if let Some(state_root_hash) = self.post_state_hash {
            let mut tracking_copy = self
                .data_access_layer
                .tracking_copy(state_root_hash)
                .expect("should not error on checkout")
                .expect("should checkout tracking copy");

            for (key, val) in data {
                tracking_copy.write(key, val);
            }

            self.commit_transforms(state_root_hash, tracking_copy.effects());
        }
        self
    }

    /// Sets gas hold config into global state.
    pub fn with_gas_hold_config(
        &mut self,
        handling: HoldBalanceHandling,
        interval: u64,
    ) -> &mut Self {
        if let Some(state_root_hash) = self.post_state_hash {
            let mut tracking_copy = self
                .data_access_layer
                .tracking_copy(state_root_hash)
                .expect("should not error on checkout")
                .expect("should checkout tracking copy");

            let registry = tracking_copy
                .get_system_entity_registry()
                .expect("should have registry");
            let mint = *registry.get("mint").expect("should have mint");
            let mint_addr = EntityAddr::new_system(mint);
            let named_keys = tracking_copy
                .get_named_keys(mint_addr)
                .expect("should have named keys");

            let mut address_generator =
                AddressGenerator::new(state_root_hash.as_ref(), Phase::System);

            // gas handling
            let uref = address_generator.new_uref(AccessRights::READ_ADD_WRITE);
            let stored_value = StoredValue::CLValue(
                CLValue::from_t(handling.tag()).expect("should turn handling tag into CLValue"),
            );

            tracking_copy
                .upsert_uref_to_named_keys(
                    mint_addr,
                    MINT_GAS_HOLD_HANDLING_KEY,
                    &named_keys,
                    uref,
                    stored_value,
                )
                .expect("should upsert gas handling");

            // gas interval
            let uref = address_generator.new_uref(AccessRights::READ_ADD_WRITE);
            let stored_value = StoredValue::CLValue(
                CLValue::from_t(interval).expect("should turn gas interval into CLValue"),
            );

            tracking_copy
                .upsert_uref_to_named_keys(
                    mint_addr,
                    MINT_GAS_HOLD_INTERVAL_KEY,
                    &named_keys,
                    uref,
                    stored_value,
                )
                .expect("should upsert gas interval");

            self.commit_transforms(state_root_hash, tracking_copy.effects());
        }
        self
    }

    /// Returns the engine state.
    pub fn get_engine_state(&self) -> &ExecutionEngineV1 {
        &self.execution_engine
    }

    /// Returns the engine state.
    pub fn data_access_layer(&self) -> &S {
        &self.data_access_layer
    }

    /// Returns the last results execs.
    pub fn get_last_exec_result(&self) -> Option<WasmV1Result> {
        self.exec_results.last().cloned()
    }

    /// Returns the owned results of a specific exec.
    pub fn get_exec_result_owned(&self, index: usize) -> Option<WasmV1Result> {
        self.exec_results.get(index).cloned()
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

    /// Expect failure of the protocol upgrade.
    pub fn expect_upgrade_failure(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let result = self
            .upgrade_results
            .last()
            .expect("Expected to be called after a system upgrade.");

        assert!(result.is_err(), "Expected Failure got {:?}", result);

        self
    }

    /// Returns the `Account` if present.
    pub fn get_account(&self, account_hash: AccountHash) -> Option<Account> {
        let stored_value = self
            .query(None, Key::Account(account_hash), &[])
            .expect("must have stored value");

        stored_value.into_account()
    }

    /// Returns the "handle payment" contract, panics if it can't be found.
    pub fn get_handle_payment_contract(&self) -> EntityWithNamedKeys {
        let hash = self
            .get_system_entity_hash(HANDLE_PAYMENT)
            .expect("should have handle payment contract");

        let handle_payment_contract = if self.chainspec.core_config.enable_addressable_entity {
            Key::addressable_entity_key(EntityKindTag::System, hash)
        } else {
            Key::Hash(hash.value())
        };
        let stored_value = self
            .query(None, handle_payment_contract, &[])
            .expect("must have stored value");
        match stored_value {
            StoredValue::Contract(contract) => {
                let named_keys = contract.named_keys().clone();
                let entity = AddressableEntity::from(contract);
                EntityWithNamedKeys::new(entity, named_keys)
            }
            StoredValue::AddressableEntity(entity) => {
                let named_keys = self.get_named_keys(EntityAddr::System(hash.value()));
                EntityWithNamedKeys::new(entity, named_keys)
            }
            _ => panic!("unhandled stored value"),
        }
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
    pub fn get_purse_balance_result_with_proofs(
        &self,
        protocol_version: ProtocolVersion,
        balance_identifier: BalanceIdentifier,
    ) -> BalanceResult {
        let balance_handling = BalanceHandling::Available;
        let proof_handling = ProofHandling::Proofs;
        let state_root_hash: Digest = self.post_state_hash.expect("should have post_state_hash");
        let request = BalanceRequest::new(
            state_root_hash,
            protocol_version,
            balance_identifier,
            balance_handling,
            proof_handling,
        );
        self.data_access_layer.balance(request)
    }

    /// Returns a `BalanceResult` for a purse using a `PublicKey`.
    pub fn get_public_key_balance_result_with_proofs(
        &self,
        protocol_version: ProtocolVersion,
        public_key: PublicKey,
    ) -> BalanceResult {
        let state_root_hash: Digest = self.post_state_hash.expect("should have post_state_hash");
        let balance_handling = BalanceHandling::Available;
        let proof_handling = ProofHandling::Proofs;
        let request = BalanceRequest::from_public_key(
            state_root_hash,
            protocol_version,
            public_key,
            balance_handling,
            proof_handling,
        );
        self.data_access_layer.balance(request)
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
            Some(StoredValue::Account(_)) => Some(AddressableEntityHash::new(account_hash.value())),
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
                let named_keys = self.get_named_keys(entity.entity_addr(entity_hash));
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
            Some(StoredValue::Account(account)) => Some(AddressableEntity::from(account)),
            Some(StoredValue::CLValue(cl_value)) => {
                let entity_key = CLValue::into_t::<Key>(cl_value).expect("must have entity key");
                match self.query(None, entity_key, &[]) {
                    Ok(StoredValue::AddressableEntity(entity)) => Some(entity),
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
        if !self.chainspec.core_config.enable_addressable_entity {
            let contract_hash = ContractHash::new(entity_hash.value());
            return self
                .get_contract(contract_hash)
                .map(AddressableEntity::from);
        }

        let entity_key = Key::addressable_entity_key(EntityKindTag::SmartContract, entity_hash);

        let value: StoredValue = match self.query(None, entity_key, &[]) {
            Ok(stored_value) => stored_value,
            Err(_) => self
                .query(
                    None,
                    Key::addressable_entity_key(EntityKindTag::System, entity_hash),
                    &[],
                )
                .ok()?,
        };

        if let StoredValue::AddressableEntity(entity) = value {
            Some(entity)
        } else {
            None
        }
    }

    /// Retrieve a Contract from global state.
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
        let key = if self.chainspec.core_config.enable_addressable_entity {
            Key::SmartContract(package_hash.value())
        } else {
            Key::Hash(package_hash.value())
        };
        let contract_value: StoredValue = self
            .query(None, key, &[])
            .expect("should have package value");

        match contract_value {
            StoredValue::ContractPackage(contract_package) => Some(contract_package.into()),
            StoredValue::SmartContract(package) => Some(package),
            _ => None,
        }
    }

    /// Returns how much gas execution consumed / used.
    pub fn exec_consumed(&self, index: usize) -> Gas {
        self.exec_results
            .get(index)
            .map(WasmV1Result::consumed)
            .unwrap()
    }

    /// Returns the `Gas` cost of the last exec.
    pub fn last_exec_gas_consumed(&self) -> Gas {
        self.exec_results
            .last()
            .map(WasmV1Result::consumed)
            .unwrap()
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

    /// Gets [`EraValidators`].
    pub fn get_era_validators(&mut self) -> EraValidators {
        let state_hash = self.get_post_state_hash();
        let request = EraValidatorsRequest::new(state_hash);
        let result = self.data_access_layer.era_validators(request);

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

        let get_bids_result = self.data_access_layer.bids(get_bids_request);

        get_bids_result.into_option().unwrap()
    }

    /// Returns named keys for an account entity by its account hash.
    pub fn get_named_keys_by_account_hash(&self, account_hash: AccountHash) -> NamedKeys {
        let entity_hash = self
            .get_entity_hash_by_account_hash(account_hash)
            .expect("must have entity hash");
        let entity_addr = EntityAddr::new_account(entity_hash.value());
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
            .data_access_layer
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        tracking_copy
            .get_named_keys(entity_addr)
            .expect("should have named keys")
    }

    /// Gets [`BTreeMap<UnbondKind, Unbond>`].
    pub fn get_unbonds(&mut self) -> BTreeMap<UnbondKind, Vec<Unbond>> {
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .data_access_layer
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let reader = tracking_copy.reader();

        let unbond_keys = reader
            .keys_with_prefix(&[KeyTag::BidAddr as u8])
            .unwrap_or_default();

        let mut ret = BTreeMap::new();

        for key in unbond_keys.into_iter() {
            if let Ok(Some(StoredValue::BidKind(BidKind::Unbond(unbond)))) = reader.read(&key) {
                let unbond_kind = unbond.unbond_kind();
                match ret.get_mut(unbond_kind) {
                    None => {
                        let _ = ret.insert(unbond_kind.clone(), vec![*unbond]);
                    }
                    Some(unbonds) => unbonds.push(*unbond),
                };
            }
        }

        ret
    }

    /// Gets [`BTreeMap<AccountHash, Vec<UnbondingPurse>>`].
    pub fn get_unbonding_purses(&mut self) -> BTreeMap<AccountHash, Vec<UnbondingPurse>> {
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .data_access_layer
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
            .data_access_layer
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
            .data_access_layer
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        let reader = tracking_copy.reader();

        reader.keys_with_prefix(&[tag as u8])
    }

    /// Gets all entry points for a given entity
    pub fn get_entry_points(&self, entity_addr: EntityAddr) -> EntryPoints {
        let state_root_hash = self.get_post_state_hash();

        let tracking_copy = self
            .data_access_layer
            .tracking_copy(state_root_hash)
            .unwrap()
            .unwrap();

        tracking_copy
            .get_v1_entry_points(entity_addr)
            .expect("must get entry points")
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

    fn system_entity_key(&self, request: SystemEntityRegistryRequest) -> Key {
        let result = self.data_access_layer.system_entity_registry(request);
        if let SystemEntityRegistryResult::Success { payload, .. } = result {
            match payload {
                SystemEntityRegistryPayload::All(_) => {
                    panic!("asked for auction, got entire registry");
                }
                SystemEntityRegistryPayload::EntityKey(key) => key,
            }
        } else {
            panic!("{:?}", result)
        }
    }

    /// Gets the [`AddressableEntityHash`] of the system auction contract, panics if it can't be
    /// found.
    pub fn get_system_auction_hash(&self) -> AddressableEntityHash {
        let state_root_hash = self.get_post_state_hash();
        let request = SystemEntityRegistryRequest::new(
            state_root_hash,
            ProtocolVersion::V2_0_0,
            SystemEntityRegistrySelector::auction(),
            self.chainspec.core_config.enable_addressable_entity,
        );
        self.system_entity_key(request)
            .into_entity_hash()
            .expect("should downcast")
    }

    /// Gets the [`AddressableEntityHash`] of the system mint contract, panics if it can't be found.
    pub fn get_system_mint_hash(&self) -> AddressableEntityHash {
        let state_root_hash = self.get_post_state_hash();
        let request = SystemEntityRegistryRequest::new(
            state_root_hash,
            ProtocolVersion::V2_0_0,
            SystemEntityRegistrySelector::mint(),
            self.chainspec.core_config.enable_addressable_entity,
        );
        self.system_entity_key(request)
            .into_entity_hash()
            .expect("should downcast")
    }

    /// Gets the [`AddressableEntityHash`] of the system handle payment contract, panics if it can't
    /// be found.
    pub fn get_system_handle_payment_hash(
        &self,
        protocol_version: ProtocolVersion,
    ) -> AddressableEntityHash {
        let state_root_hash = self.get_post_state_hash();
        let request = SystemEntityRegistryRequest::new(
            state_root_hash,
            protocol_version,
            SystemEntityRegistrySelector::handle_payment(),
            self.chainspec.core_config.enable_addressable_entity,
        );
        self.system_entity_key(request)
            .into_entity_hash()
            .expect("should downcast")
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
            .with_protocol_version(ProtocolVersion::V2_0_0)
            .with_runtime_config(self.native_runtime_config())
            .with_run_auction(true);

        for _ in 0..num_eras {
            let state_hash = self.get_post_state_hash();
            let step_request = step_request_builder
                .clone()
                .with_parent_state_hash(state_hash)
                .with_next_era_id(self.get_era().successor())
                .build();

            match self.step(step_request) {
                StepResult::RootNotFound => panic!("Root not found {:?}", state_hash),
                StepResult::Failure(err) => panic!("{:?}", err),
                StepResult::Success { .. } => {
                    // noop
                }
            }
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

    /// Returns an initialized step request builder.
    pub fn step_request_builder(&mut self) -> StepRequestBuilder {
        StepRequestBuilder::new()
            .with_parent_state_hash(self.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V2_0_0)
            .with_runtime_config(self.native_runtime_config())
    }

    /// Returns a trie by hash.
    pub fn get_trie(&mut self, state_hash: Digest) -> Option<Trie<Key, StoredValue>> {
        let req = TrieRequest::new(state_hash, None);
        self.data_access_layer()
            .trie(req)
            .into_raw()
            .unwrap()
            .map(|bytes| bytesrepr::deserialize(bytes.into_inner().into()).unwrap())
    }

    /// Returns the costs related to interacting with the auction system contract.
    pub fn get_auction_costs(&self) -> AuctionCosts {
        *self.chainspec.system_costs_config.auction_costs()
    }

    /// Returns the costs related to interacting with the mint system contract.
    pub fn get_mint_costs(&self) -> MintCosts {
        *self.chainspec.system_costs_config.mint_costs()
    }

    /// Returns the costs related to interacting with the handle payment system contract.
    pub fn get_handle_payment_costs(&self) -> HandlePaymentCosts {
        *self.chainspec.system_costs_config.handle_payment_costs()
    }

    /// Commits a prune of leaf nodes from the tip of the merkle trie.
    pub fn commit_prune(&mut self, prune_config: PruneRequest) -> &mut Self {
        let result = self.data_access_layer.prune(prune_config);

        if let PruneResult::Success {
            post_state_hash,
            effects,
        } = &result
        {
            self.post_state_hash = Some(*post_state_hash);
            self.effects.push(effects.clone());
        }

        self.prune_results.push(result);
        self
    }

    /// Returns a `Result` containing a [`PruneResult`].
    pub fn get_prune_result(&self, index: usize) -> Option<&PruneResult> {
        self.prune_results.get(index)
    }

    /// Expects a prune success.
    pub fn expect_prune_success(&mut self) -> &mut Self {
        // Check first result, as only first result is interesting for a simple test
        let result = self
            .prune_results
            .last()
            .expect("Expected to be called after a system upgrade.");

        match result {
            PruneResult::RootNotFound => panic!("Root not found"),
            PruneResult::MissingKey => panic!("Does not exists"),
            PruneResult::Failure(tce) => {
                panic!("{:?}", tce);
            }
            PruneResult::Success { .. } => {}
        }

        self
    }

    /// Calculates refunded amount from a last execution request.
    pub fn calculate_refund_amount(&self, payment_amount: U512) -> U512 {
        let gas_amount = Motes::from_gas(self.last_exec_gas_consumed(), DEFAULT_GAS_PRICE)
            .expect("should create motes from gas");

        let refund_ratio = match self.chainspec.core_config.refund_handling {
            RefundHandling::Refund { refund_ratio } | RefundHandling::Burn { refund_ratio } => {
                refund_ratio
            }
            RefundHandling::NoRefund => Ratio::zero(),
        };

        let (numer, denom) = refund_ratio.into();
        let refund_ratio = Ratio::new_raw(U512::from(numer), U512::from(denom));

        // amount declared to be paid in payment code MINUS gas spent in last execution.
        let refundable_amount = Ratio::from(payment_amount) - Ratio::from(gas_amount.value());
        (refundable_amount * refund_ratio).to_integer()
    }
}
