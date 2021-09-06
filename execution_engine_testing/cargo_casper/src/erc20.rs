use once_cell::sync::Lazy;

use crate::{
    common::{CL_CONTRACT, CL_ENGINE_TEST_SUPPORT, CL_TYPES},
    dependency::Dependency,
};

pub const MAIN_RS_CONTENTS: &str = include_str!("../resources/erc20/main.rs.in");
pub const INTEGRATION_TESTS_RS_CONTENTS: &str =
    include_str!("../resources/erc20/integration_tests.rs.in");
pub const TEST_FIXTURE_RS_CONTENTS: &str = include_str!("../resources/erc20/test_fixture.rs.in");
pub const MAKEFILE_CONTENTS: &str = include_str!("../resources/erc20/Makefile.in");

static CL_ERC20: Lazy<Dependency> = Lazy::new(|| Dependency::new("casper-erc20", "0.1.0", ""));

pub static CONTRACT_DEPENDENCIES: Lazy<String> = Lazy::new(|| {
    format!(
        "{}{}{}",
        CL_CONTRACT.display_with_features(true, vec![]),
        CL_ERC20.display_with_features(true, vec![]),
        CL_TYPES.display_with_features(true, vec![]),
    )
});

pub static TEST_DEPENDENCIES: Lazy<String> = Lazy::new(|| {
    format!(
        "{}{}{}{}{}{}",
        Dependency::new("base64", "0.13.0", "").display_with_features(true, vec![]),
        Dependency::new("blake2", "0.9.2", "").display_with_features(true, vec![]),
        CL_ENGINE_TEST_SUPPORT.display_with_features(true, vec!["test-support"]),
        CL_ERC20.display_with_features(true, vec!["std"]),
        CL_TYPES.display_with_features(true, vec!["std"]),
        Dependency::new("hex", "0.4.3", "").display_with_features(true, vec![]),
    )
});
