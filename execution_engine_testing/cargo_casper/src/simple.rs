use once_cell::sync::Lazy;

use crate::common::{CL_CONTRACT, CL_ENGINE_TEST_SUPPORT, CL_TYPES};

pub const MAIN_RS_CONTENTS: &str = include_str!("../resources/simple/main.rs.in");
pub const INTEGRATION_TESTS_RS_CONTENTS: &str =
    include_str!("../resources/simple/integration_tests.rs.in");
pub const MAKEFILE_CONTENTS: &str = include_str!("../resources/simple/Makefile.in");

pub static CONTRACT_DEPENDENCIES: Lazy<String> = Lazy::new(|| {
    format!(
        "{}{}",
        CL_CONTRACT.display_with_features(true, vec![]),
        CL_TYPES.display_with_features(true, vec![]),
    )
});

pub static TEST_DEPENDENCIES: Lazy<String> = Lazy::new(|| {
    format!(
        "{}{}{}",
        CL_CONTRACT.display_with_features(false, vec!["std", "test-support"]),
        CL_ENGINE_TEST_SUPPORT.display_with_features(true, vec!["test-support"]),
        CL_TYPES.display_with_features(false, vec!["std"]),
    )
});
