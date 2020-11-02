#![allow(clippy::wildcard_imports)]

use lazy_static::lazy_static;
use regex::Regex;

use crate::dependent_file::DependentFile;

lazy_static! {
    pub static ref MANIFEST_NAME_REGEX: Regex = Regex::new(r#"(?m)(^name = )"([^"]+)"#).unwrap();
    pub static ref MANIFEST_VERSION_REGEX: Regex =
        Regex::new(r#"(?m)(^version = )"([^"]+)"#).unwrap();
    pub static ref PACKAGE_JSON_NAME_REGEX: Regex =
        Regex::new(r#"(?m)(^  "name": )"([^"]+)"#).unwrap();
    pub static ref PACKAGE_JSON_VERSION_REGEX: Regex =
        Regex::new(r#"(?m)(^  "version": )"([^"]+)"#).unwrap();
}

fn replacement(updated_version: &str) -> String {
    format!(r#"$1"{}"#, updated_version)
}

fn replacement_with_slash(updated_version: &str) -> String {
    format!(r#"$1/{}"#, updated_version)
}

pub mod types {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![
                DependentFile::new(
                    "client/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "execution_engine/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "grpc/cargo_casper/src/common.rs",
                    Regex::new(r#"(?m)("casper-types",\s*)"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "grpc/server/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "grpc/test_support/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "node/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "smart_contracts/contract/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
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
            ]
        };
    }
}

pub mod execution_engine {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![
                DependentFile::new(
                    "client/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-execution-engine = \{[^\}]*version = )"(?:[^"]+)"#)
                        .unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "grpc/server/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-execution-engine = \{[^\}]*version = )"(?:[^"]+)"#)
                        .unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "grpc/test_support/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-execution-engine = \{[^\}]*version = )"(?:[^"]+)"#)
                        .unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "node/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-execution-engine = \{[^\}]*version = )"(?:[^"]+)"#)
                        .unwrap(),
                    replacement,
                ),
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
            ]
        };
    }
}

pub mod node_macros {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![
                DependentFile::new(
                    "node/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-node-macros = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "node_macros/Cargo.toml",
                    MANIFEST_VERSION_REGEX.clone(),
                    replacement,
                ),
                DependentFile::new(
                    "node_macros/src/lib.rs",
                    Regex::new(
                        r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-node-macros)/(?:[^"]+)"#,
                    )
                    .unwrap(),
                    replacement_with_slash,
                ),
            ]
        };
    }
}

pub mod node {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![
                DependentFile::new(
                    "client/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-node = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
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
        };
    }
}

pub mod grpc_server {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![
                DependentFile::new(
                    "grpc/server/Cargo.toml",
                    MANIFEST_VERSION_REGEX.clone(),
                    replacement,
                ),
                DependentFile::new(
                    "grpc/test_support/Cargo.toml",
                    Regex::new(
                        r#"(?m)(^casper-engine-grpc-server = \{[^\}]*version = )"(?:[^"]+)"#,
                    )
                    .unwrap(),
                    replacement,
                ),
            ]
        };
    }
}

pub mod client {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![DependentFile::new(
                "client/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            )]
        };
    }
}

pub mod smart_contracts_contract {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![
                DependentFile::new(
                    "grpc/cargo_casper/src/common.rs",
                    Regex::new(r#"(?m)("casper-contract",\s*)"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "grpc/test_support/Cargo.toml",
                    Regex::new(r#"(?m)(^casper-contract = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                    replacement,
                ),
                DependentFile::new(
                    "smart_contracts/contract/Cargo.toml",
                    MANIFEST_VERSION_REGEX.clone(),
                    replacement,
                ),
                DependentFile::new(
                    "smart_contracts/contract/src/lib.rs",
                    Regex::new(r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-contract)/(?:[^"]+)"#).unwrap(),
                    replacement_with_slash,
                ),
            ]
        };
    }
}

pub mod smart_contracts_contract_as {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![
                DependentFile::new(
                    "smart_contracts/contract_as/package.json",
                    PACKAGE_JSON_VERSION_REGEX.clone(),
                    replacement,
                ),
                DependentFile::new(
                    "smart_contracts/contract_as/package-lock.json",
                    PACKAGE_JSON_VERSION_REGEX.clone(),
                    replacement,
                ),
            ]
        };
    }
}

pub mod grpc_test_support {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![
                DependentFile::new(
                    "grpc/cargo_casper/src/tests_package.rs",
                    Regex::new(r#"(?m)("casper-engine-test-support",\s*)"(?:[^"]+)"#).unwrap(),
                    cargo_casper_src_test_package_rs_replacement,
                ),
                DependentFile::new(
                    "grpc/test_support/Cargo.toml",
                    MANIFEST_VERSION_REGEX.clone(),
                    replacement,
                ),
                DependentFile::new(
                    "grpc/test_support/src/lib.rs",
                    Regex::new(r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-engine-test-support)/(?:[^"]+)"#).unwrap(),
                    replacement_with_slash,
                ),
            ]
        };
    }

    fn cargo_casper_src_test_package_rs_replacement(updated_version: &str) -> String {
        format!(r#"$1"{}"#, updated_version)
    }
}

pub mod grpc_cargo_casper {
    use super::*;

    lazy_static! {
        pub static ref DEPENDENT_FILES: Vec<DependentFile> = {
            vec![DependentFile::new(
                "grpc/cargo_casper/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            )]
        };
    }
}
