#![allow(clippy::wildcard_imports)]

use once_cell::sync::Lazy;
use regex::Regex;

use crate::dependent_file::DependentFile;

pub static MANIFEST_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^name = )"([^"]+)"#).unwrap());
pub static MANIFEST_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^version = )"([^"]+)"#).unwrap());

fn replacement(updated_version: &str) -> String {
    format!(r#"$1"{}"#, updated_version)
}

fn replacement_with_slash(updated_version: &str) -> String {
    format!(r#"$1/{}"#, updated_version)
}

pub static TYPES_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap());

pub mod binary_port {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "binary_port/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "node/Cargo.toml",
                Regex::new(r#"(?m)(^casper-binary-port = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                replacement,
            ),
        ]
    });
}

pub mod types {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "types/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "types/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-types)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
            DependentFile::new(
                "binary_port/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "storage/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine_testing/test_support/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new("node/Cargo.toml", TYPES_VERSION_REGEX.clone(), replacement),
            DependentFile::new(
                "smart_contracts/contract/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm_host/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm_interface/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasmer_backend/Cargo.toml",
                TYPES_VERSION_REGEX.clone(),
                replacement,
            ),
        ]
    });
}

pub static STORAGE_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^casper-storage = \{[^\}]*version = )"(?:[^"]+)"#).unwrap());
pub mod storage {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "storage/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "storage/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-storage)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
            DependentFile::new(
                "execution_engine/Cargo.toml",
                STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine_testing/test_support/Cargo.toml",
                STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "node/Cargo.toml",
                STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm/Cargo.toml",
                STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm_host/Cargo.toml",
                STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm_interface/Cargo.toml",
                STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasmer_backend/Cargo.toml",
                STORAGE_VERSION_REGEX.clone(),
                replacement,
            ),
        ]
    });
}

pub static EXECUTION_ENGINE_VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?m)(^casper-execution-engine = \{[^\}]*version = )"(?:[^"]+)"#).unwrap()
});
pub mod execution_engine {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "execution_engine/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine/src/lib.rs",
                Regex::new(r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-execution-engine)/(?:[^"]+)"#).unwrap(),
                replacement_with_slash,
            ),
                DependentFile::new(
                    "execution_engine_testing/test_support/Cargo.toml",
                    EXECUTION_ENGINE_VERSION_REGEX.clone(),
                    replacement,
                ),
                DependentFile::new(
                    "node/Cargo.toml",
                    EXECUTION_ENGINE_VERSION_REGEX.clone(),
                    replacement,
                ),
            DependentFile::new(
                "executor/wasm/Cargo.toml",
                EXECUTION_ENGINE_VERSION_REGEX.clone(),
                replacement,
            ),
            ]
    });
}

pub mod execution_engine_testing_test_support {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "execution_engine_testing/test_support/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine_testing/test_support/src/lib.rs",
                Regex::new(r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-engine-test-support)/(?:[^"]+)"#).unwrap(),
                replacement_with_slash,
            ),
        ]
    });
}

pub mod node {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "node/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "node/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-node)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
        ]
    });
}

pub mod smart_contracts_contract {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "smart_contracts/contract/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/contract/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-contract)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
        ]
    });
}

pub static SMART_CONTRACTS_SDK_SYS_VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?m)(^casper-contract-sdk-sys = \{[^\}]*version = )"(?:[^"]+)"#).unwrap()
});

pub mod smart_contracts_sdk_sys {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "smart_contracts/sdk_sys/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm_common/Cargo.toml",
                SMART_CONTRACTS_SDK_SYS_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasmer_backend/Cargo.toml",
                SMART_CONTRACTS_SDK_SYS_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/macros/Cargo.toml",
                SMART_CONTRACTS_SDK_SYS_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/sdk/Cargo.toml",
                SMART_CONTRACTS_SDK_SYS_VERSION_REGEX.clone(),
                replacement,
            ),
        ]
    });
}

pub mod smart_contracts_sdk {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "smart_contracts/sdk/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/sdk_codegen/Cargo.toml",
                Regex::new(r#"(?m)(^casper-contract-sdk = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                replacement,
            ),
        ]
    });
}

pub mod smart_contracts_sdk_codegen {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![DependentFile::new(
            "smart_contracts/sdk_codegen/Cargo.toml",
            MANIFEST_VERSION_REGEX.clone(),
            replacement,
        )]
    });
}
pub mod smart_contracts_macros {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "smart_contracts/macros/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/sdk/Cargo.toml",
                Regex::new(r#"(?m)(^casper-contract-macros = \{[^\}]*version = )"(?:[^"]+)"#)
                    .unwrap(),
                replacement,
            ),
        ]
    });
}

pub static EXECUTOR_WASM_COMMON_VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?m)(^casper-executor-wasm-common = \{[^\}]*version = )"(?:[^"]+)"#).unwrap()
});
pub mod executor_wasm_common {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "executor/wasm_common/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm/Cargo.toml",
                EXECUTOR_WASM_COMMON_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm_host/Cargo.toml",
                EXECUTOR_WASM_COMMON_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm_interface/Cargo.toml",
                EXECUTOR_WASM_COMMON_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasmer_backend/Cargo.toml",
                EXECUTOR_WASM_COMMON_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/macros/Cargo.toml",
                EXECUTOR_WASM_COMMON_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "smart_contracts/sdk/Cargo.toml",
                EXECUTOR_WASM_COMMON_VERSION_REGEX.clone(),
                replacement,
            ),
        ]
    });
}

pub static EXECUTOR_WASM_INTERFACE_VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?m)(^casper-executor-wasm-interface = \{[^\}]*version = )"(?:[^"]+)"#).unwrap()
});
pub mod executor_wasm_interface {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "executor/wasm_interface/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm/Cargo.toml",
                EXECUTOR_WASM_INTERFACE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm_host/Cargo.toml",
                EXECUTOR_WASM_INTERFACE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasmer_backend/Cargo.toml",
                EXECUTOR_WASM_INTERFACE_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "node/Cargo.toml",
                EXECUTOR_WASM_INTERFACE_VERSION_REGEX.clone(),
                replacement,
            ),
        ]
    });
}

pub static EXECUTOR_WASM_HOST_VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?m)(^casper-executor-wasm-host = \{[^\}]*version = )"(?:[^"]+)"#).unwrap()
});
pub mod executor_wasm_host {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "executor/wasm_host/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm/Cargo.toml",
                EXECUTOR_WASM_HOST_VERSION_REGEX.clone(),
                replacement,
            ),
        ]
    });
}

pub static EXECUTOR_WASMER_BACKEND_VERSION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?m)(^casper-executor-wasmer-backend = \{[^\}]*version = )"(?:[^"]+)"#).unwrap()
});
pub mod executor_wasmer_backend {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "executor/wasmer_backend/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "executor/wasm/Cargo.toml",
                EXECUTOR_WASMER_BACKEND_VERSION_REGEX.clone(),
                replacement,
            ),
        ]
    });
}

pub mod executor_wasm {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "executor/wasm/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "node/Cargo.toml",
                Regex::new(r#"(?m)(^casper-executor-wasm = \{[^\}]*version = )"(?:[^"]+)"#)
                    .unwrap(),
                replacement,
            ),
        ]
    });
}
