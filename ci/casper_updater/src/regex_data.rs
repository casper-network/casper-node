#![allow(clippy::wildcard_imports)]

use once_cell::sync::Lazy;
use regex::Regex;

use crate::dependent_file::DependentFile;

pub static MANIFEST_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^name = )"([^"]+)"#).unwrap());
pub static MANIFEST_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^version = )"([^"]+)"#).unwrap());
pub static PACKAGE_JSON_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^  "name": )"([^"]+)"#).unwrap());
pub static PACKAGE_JSON_VERSION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?m)(^  "version": )"([^"]+)"#).unwrap());

fn replacement(updated_version: &str) -> String {
    format!(r#"$1"{}"#, updated_version)
}

fn replacement_with_slash(updated_version: &str) -> String {
    format!(r#"$1/{}"#, updated_version)
}

pub mod types {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "execution_engine/Cargo.toml",
                Regex::new(r#"(?m)(^casper-types = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine_testing/test_support/Cargo.toml",
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
    });
}

pub mod hashing {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "execution_engine/Cargo.toml",
                Regex::new(r#"(?m)(^casper-hashing = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                replacement,
            ),
            DependentFile::new(
                "execution_engine_testing/test_support/Cargo.toml",
                Regex::new(r#"(?m)(^casper-hashing = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                replacement,
            ),
            DependentFile::new(
                "node/Cargo.toml",
                Regex::new(r#"(?m)(^casper-hashing = \{[^\}]*version = )"(?:[^"]+)"#).unwrap(),
                replacement,
            ),
            DependentFile::new(
                "hashing/Cargo.toml",
                MANIFEST_VERSION_REGEX.clone(),
                replacement,
            ),
            DependentFile::new(
                "hashing/src/lib.rs",
                Regex::new(
                    r#"(?m)(#!\[doc\(html_root_url = "https://docs.rs/casper-hashing)/(?:[^"]+)"#,
                )
                .unwrap(),
                replacement_with_slash,
            ),
        ]
    });
}

pub mod execution_engine {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
                DependentFile::new(
                    "execution_engine_testing/test_support/Cargo.toml",
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
    });
}

pub mod node_macros {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
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

pub mod smart_contracts_contract_as {
    use super::*;

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
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

pub mod chainspec_protocol_version {
    use super::*;

    pub static REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?m)(^version = )'([^']+)"#).unwrap());

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![
            DependentFile::new(
                "resources/production/chainspec.toml",
                REGEX.clone(),
                chainspec_toml_replacement,
            ),
            DependentFile::new(
                "node/src/components/rpc_server/rpcs/docs.rs",
                Regex::new(r#"(?m)(DOCS_EXAMPLE_PROTOCOL_VERSION: ProtocolVersion =\s*ProtocolVersion::from_parts)\((\d+,\s*\d+,\s*\d+)\)"#).unwrap(),
                rpcs_docs_rs_replacement,
            ),
        ]
    });

    fn chainspec_toml_replacement(updated_version: &str) -> String {
        format!(r#"$1'{}"#, updated_version)
    }

    fn rpcs_docs_rs_replacement(updated_version: &str) -> String {
        format!(r#"$1({})"#, updated_version.replace('.', ", "))
    }
}

pub mod chainspec_activation_point {
    use super::*;

    pub static REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"(?m)(^activation_point =) (.+)"#).unwrap());

    pub static DEPENDENT_FILES: Lazy<Vec<DependentFile>> = Lazy::new(|| {
        vec![DependentFile::new(
            "resources/production/chainspec.toml",
            REGEX.clone(),
            chainspec_toml_replacement,
        )]
    });

    fn chainspec_toml_replacement(updated_activation_point: &str) -> String {
        format!(r#"$1 {}"#, updated_activation_point)
    }
}
