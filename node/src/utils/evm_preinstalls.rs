//! Loads content-addressed EVM runtimes from a selected protocol package.

use std::{fs, path::Path};

use alloy_primitives::keccak256;
use anyhow::{anyhow, ensure};
use casper_types::{bytesrepr::Bytes, evm};

/// Reads and validates listed runtime files when EVM is enabled.
pub(crate) fn load_preinstall_bytecodes(
    protocol_dir: &Path,
    evm_enabled: bool,
    preinstalls: &[(evm::Address, evm::Hash)],
) -> anyhow::Result<Vec<(evm::Hash, Bytes)>> {
    let mut bytecodes = Vec::new();
    if !evm_enabled {
        return Ok(bytecodes);
    }
    for &(address, expected) in preinstalls {
        if bytecodes.iter().any(|(hash, _)| *hash == expected) {
            continue;
        }
        let path = protocol_dir
            .join("preinstalls")
            .join(format!("{}.bin", expected.to_hex_string()));
        let code = fs::read(&path).map_err(|error| {
            anyhow!("EVM preinstall at {address}: cannot read {path:?}, expected runtime hash {expected}: {error}")
        })?;
        ensure!(
            !code.is_empty(),
            "EVM preinstall at {address}: empty runtime bytecode in {path:?}, expected {expected}"
        );
        let actual = evm::Hash::new(keccak256(&code).0);
        ensure!(
            actual == expected,
            "EVM preinstall at {address}: runtime hash mismatch in {path:?}: expected {expected}, actual {actual}"
        );
        bytecodes.push((expected, code.into()));
    }
    Ok(bytecodes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const CODE: &[u8] = &[0x00];

    fn fixture() -> (tempfile::TempDir, Vec<(evm::Address, evm::Hash)>, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let expected = evm::Hash::new(keccak256(CODE).0);
        let preinstalls = vec![
            (evm::Address::new([1; 20]), expected),
            (evm::Address::new([2; 20]), expected),
        ];
        fs::create_dir(dir.path().join("preinstalls")).unwrap();
        let path = dir
            .path()
            .join("preinstalls")
            .join(format!("{}.bin", expected.to_hex_string()));
        (dir, preinstalls, path)
    }

    #[test]
    fn loads_shared_bytecode_independently_of_protocol_directory() {
        for _ in 0..2 {
            let (dir, preinstalls, path) = fixture();
            fs::write(path, CODE).unwrap();
            let bytecodes = load_preinstall_bytecodes(dir.path(), true, &preinstalls).unwrap();
            assert_eq!(bytecodes, vec![(preinstalls[0].1, Bytes::from(CODE))]);
        }
    }

    #[test]
    fn missing_and_unreadable_files_report_the_address_path_and_hash() {
        let (dir, preinstalls, path) = fixture();
        for unreadable in [false, true] {
            if unreadable {
                // A directory cannot be read as a bytecode file, even when run as root.
                fs::create_dir(&path).unwrap();
            }
            let message = load_preinstall_bytecodes(dir.path(), true, &preinstalls)
                .unwrap_err()
                .to_string();
            assert!(message.contains("cannot read"));
            assert!(message.contains(&preinstalls[0].0.to_string()));
            assert!(message.contains(&path.display().to_string()));
            assert!(message.contains(&preinstalls[0].1.to_string()));
        }
    }

    #[test]
    fn rejects_empty_and_mismatched_bytecode_and_revalidates_changes() {
        let (dir, preinstalls, path) = fixture();
        fs::write(&path, CODE).unwrap();
        assert!(load_preinstall_bytecodes(dir.path(), true, &preinstalls).is_ok());
        for (invalid, diagnostic) in [
            (&[][..], "empty runtime bytecode"),
            (&[0x01][..], "runtime hash mismatch"),
        ] {
            fs::write(&path, invalid).unwrap();
            let message = load_preinstall_bytecodes(dir.path(), true, &preinstalls)
                .unwrap_err()
                .to_string();
            assert!(message.contains(diagnostic));
            assert!(message.contains(&path.display().to_string()));
            assert!(message.contains(&preinstalls[0].0.to_string()));
            assert!(message.contains(&preinstalls[0].1.to_string()));
            if !invalid.is_empty() {
                assert!(message.contains(&evm::Hash::new(keccak256(invalid).0).to_string()));
            }
            fs::write(&path, CODE).unwrap();
            assert!(load_preinstall_bytecodes(dir.path(), true, &preinstalls).is_ok());
        }
        fs::remove_file(&path).unwrap();
        assert!(load_preinstall_bytecodes(dir.path(), true, &preinstalls).is_err());
    }

    #[test]
    fn disabled_evm_or_empty_list_requires_no_asset_directory() {
        let dir = tempfile::tempdir().unwrap();
        let preinstalls = [(evm::Address::new([1; 20]), evm::Hash::new([2; 32]))];
        for (enabled, entries) in [(false, preinstalls.as_slice()), (true, &[])] {
            assert!(load_preinstall_bytecodes(dir.path(), enabled, entries)
                .unwrap()
                .is_empty());
        }
    }
}
