use std::{
    env,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use fs_extra::dir;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tempfile::TempDir;

use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_execution_engine::engine_state::EngineConfig;
use casper_storage::data_access_layer::GenesisRequest;
#[cfg(test)]
use casper_types::{AccessRights, Key, URef};
use casper_types::{Digest, ProtocolVersion};

pub const RELEASE_1_2_0: &str = "release_1_2_0";
pub const RELEASE_1_3_1: &str = "release_1_3_1";
pub const RELEASE_1_4_2: &str = "release_1_4_2";
pub const RELEASE_1_4_3: &str = "release_1_4_3";
pub const RELEASE_1_4_4: &str = "release_1_4_4";
pub const RELEASE_1_4_5: &str = "release_1_4_5";
const STATE_JSON_FILE: &str = "state.json";
const FIXTURES_DIRECTORY: &str = "fixtures";
const GENESIS_PROTOCOL_VERSION_FIELD: &str = "protocol_version";

#[cfg(test)]
const RUN_FIXTURE_GENERATORS_ENV: &str = "RUN_FIXTURE_GENERATORS";

#[cfg(test)]
pub(crate) fn is_fixture_generator_enabled() -> bool {
    env::var_os(RUN_FIXTURE_GENERATORS_ENV).is_some()
}

/// This is a special place in the global state where fixture contains a registry.
#[cfg(test)]
pub(crate) const CONTRACT_REGISTRY_SPECIAL_ADDRESS: Key =
    Key::URef(URef::new([0u8; 32], AccessRights::all()));

fn path_to_lmdb_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES_DIRECTORY)
}

/// Contains serialized genesis config.
#[derive(Serialize, Deserialize)]
pub struct LmdbFixtureState {
    /// Serializes as unstructured JSON value because [`GenesisRequest`] might change over time
    /// and likely old fixture might not deserialize cleanly in the future.
    pub genesis_request: serde_json::Value,
    pub post_state_hash: Digest,
}

impl LmdbFixtureState {
    pub fn genesis_protocol_version(&self) -> ProtocolVersion {
        serde_json::from_value(
            self.genesis_request
                .get(GENESIS_PROTOCOL_VERSION_FIELD)
                .cloned()
                .unwrap(),
        )
        .expect("should have protocol version field")
    }
}

/// Creates a [`LmdbWasmTestBuilder`] from a named fixture directory.
///
/// As part of this process a new temporary directory will be created to store LMDB files from given
/// fixture, and a builder will be created using it.
///
/// This function returns a triple of the builder, a [`LmdbFixtureState`] which contains serialized
/// genesis request for given fixture, and a temporary directory which has to be kept in scope.
pub fn builder_from_global_state_fixture(
    fixture_name: &str,
) -> (LmdbWasmTestBuilder, LmdbFixtureState, TempDir) {
    let source = path_to_lmdb_fixtures().join(fixture_name);
    let to = tempfile::tempdir().expect("should create temp dir");
    fs_extra::copy_items(&[source], &to, &dir::CopyOptions::default())
        .expect("should copy global state fixture");

    let path_to_state = to.path().join(fixture_name).join(STATE_JSON_FILE);
    let lmdb_fixture_state: LmdbFixtureState =
        serde_json::from_reader(File::open(path_to_state).unwrap()).unwrap();
    let path_to_gs = to.path().join(fixture_name);

    (
        LmdbWasmTestBuilder::open(
            &path_to_gs,
            EngineConfig::default(),
            lmdb_fixture_state.post_state_hash,
        ),
        lmdb_fixture_state,
        to,
    )
}

/// Creates a new fixture with a name.
///
/// This process is currently manual. The process to do this is to check out a release branch, call
/// this function to generate (i.e. `generate_fixture("release_1_3_0")`) and persist it in version
/// control.
pub fn generate_fixture(
    name: &str,
    genesis_request: GenesisRequest,
    post_genesis_setup: impl FnOnce(&mut LmdbWasmTestBuilder),
) -> Result<(), Box<dyn std::error::Error>> {
    let lmdb_fixtures_root = path_to_lmdb_fixtures();
    let fixture_root = lmdb_fixtures_root.join(name);

    let path_to_data_lmdb = fixture_root.join("global_state").join("data.lmdb");
    if path_to_data_lmdb.exists() {
        eprintln!(
            "Lmdb fixture located at {} already exists. If you need to re-generate a fixture to ensure a serialization \
            changes are backwards compatible please make sure you are running a specific version, or a past commit. \
            Skipping.",
            path_to_data_lmdb.display()
        );
        return Ok(());
    }

    let engine_config = EngineConfig::default();
    let mut builder = LmdbWasmTestBuilder::new_with_config(&fixture_root, engine_config);

    builder.run_genesis(genesis_request.clone());

    // You can customize the fixture post genesis with a callable.
    post_genesis_setup(&mut builder);

    let post_state_hash = builder.get_post_state_hash();

    let genesis_request_json = json!({
        GENESIS_PROTOCOL_VERSION_FIELD: genesis_request.protocol_version(),
    });

    let state = LmdbFixtureState {
        genesis_request: genesis_request_json,
        post_state_hash,
    };
    let serialized_state = serde_json::to_string_pretty(&state)?;

    let path_to_state_file = fixture_root.join(STATE_JSON_FILE);

    let mut f = File::create(path_to_state_file)?;
    f.write_all(serialized_state.as_bytes())?;
    Ok(())
}
