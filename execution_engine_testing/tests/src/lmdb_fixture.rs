use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use fs_extra::dir;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use casper_engine_test_support::{LmdbWasmTestBuilder, PRODUCTION_PATH};
use casper_execution_engine::core::engine_state::run_genesis_request::RunGenesisRequest;
use casper_hashing::Digest;
use casper_types::ProtocolVersion;

pub const RELEASE_1_2_0: &str = "release_1_2_0";
pub const RELEASE_1_3_1: &str = "release_1_3_1";
const STATE_JSON_FILE: &str = "state.json";
const FIXTURES_DIRECTORY: &str = "fixtures";
const GENESIS_PROTOCOL_VERSION_FIELD: &str = "protocol_version";

fn path_to_lmdb_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES_DIRECTORY)
}

/// Contains serialized genesis config.
#[derive(Serialize, Deserialize)]
pub struct LmdbFixtureState {
    /// Serializes as unstructured JSON value because [`RunGenesisRequest`] might change over time
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
        serde_json::from_reader(File::open(&path_to_state).unwrap()).unwrap();
    let path_to_gs = to.path().join(fixture_name);
    (
        LmdbWasmTestBuilder::open(
            &path_to_gs,
            &*PRODUCTION_PATH,
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
    _genesis_request: RunGenesisRequest,
    post_genesis_setup: impl FnOnce(&mut LmdbWasmTestBuilder),
) -> Result<(), Box<dyn std::error::Error>> {
    let lmdb_fixtures_root = path_to_lmdb_fixtures();
    let fixture_root = lmdb_fixtures_root.join(name);

    let mut builder = LmdbWasmTestBuilder::new_with_config(&fixture_root, &*PRODUCTION_PATH);

    builder.run_genesis_with_default_genesis_accounts();

    // You can customize the fixture post genesis with a callable.
    post_genesis_setup(&mut builder);

    let post_state_hash = builder.get_post_state_hash();

    let state = LmdbFixtureState {
        genesis_request: serde_json::to_value(builder.get_genesis_request())?,
        post_state_hash,
    };
    let serialized_state = serde_json::to_string_pretty(&state)?;
    let mut f = File::create(&fixture_root.join(STATE_JSON_FILE))?;
    f.write_all(serialized_state.as_bytes())?;
    Ok(())
}
