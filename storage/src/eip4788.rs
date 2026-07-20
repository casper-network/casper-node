//! Storage support for the EIP-4788 beacon block roots predeploy.

use alloy_primitives::keccak256;
use casper_types::{evm, BlockGlobalAddr, BlockHash, CLValue, CLValueError, Digest, Key};

/// EIP-4788 beacon roots contract address.
pub const BEACON_ROOTS_ADDRESS: evm::Address = evm::Address::new([
    0x00, 0x0f, 0x3d, 0xf6, 0xd7, 0x32, 0x80, 0x7e, 0xf1, 0x31, 0x9f, 0xb7, 0xb8, 0xbb, 0x85, 0x22,
    0xd0, 0xbe, 0xac, 0x02,
]);

/// Number of timestamp and block-root slots maintained by EIP-4788.
pub const HISTORY_BUFFER_LENGTH: u64 = 8_191;

/// Prague EIP-4788 beacon roots runtime bytecode.
pub const BEACON_ROOTS_CODE: &[u8] = &[
    0x33, 0x73, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0x14, 0x60, 0x4d, 0x57, 0x60, 0x20, 0x36, 0x14, 0x60, 0x24,
    0x57, 0x5f, 0x5f, 0xfd, 0x5b, 0x5f, 0x35, 0x80, 0x15, 0x60, 0x49, 0x57, 0x62, 0x00, 0x1f, 0xff,
    0x81, 0x06, 0x90, 0x81, 0x54, 0x14, 0x60, 0x3c, 0x57, 0x5f, 0x5f, 0xfd, 0x5b, 0x62, 0x00, 0x1f,
    0xff, 0x01, 0x54, 0x5f, 0x52, 0x60, 0x20, 0x5f, 0xf3, 0x5b, 0x5f, 0x5f, 0xfd, 0x5b, 0x62, 0x00,
    0x1f, 0xff, 0x42, 0x06, 0x42, 0x81, 0x55, 0x5f, 0x35, 0x90, 0x62, 0x00, 0x1f, 0xff, 0x01, 0x55,
    0x00,
];

/// Returns the EIP-4788 ring-buffer slot for `timestamp_secs`.
pub const fn slot_for_timestamp(timestamp_secs: u64) -> u64 {
    timestamp_secs % HISTORY_BUFFER_LENGTH
}

/// Returns the Global State key containing the EIP-4788 record for `timestamp_secs`.
pub(crate) fn parent_hash_key(timestamp_secs: u64) -> Key {
    Key::BlockGlobal(BlockGlobalAddr::BlockParentHash {
        slot: slot_for_timestamp(timestamp_secs),
    })
}

/// Returns the CLValue used to persist an EIP-4788 timestamp and parent block hash.
pub(crate) fn parent_hash_value(
    timestamp_secs: u64,
    parent_hash: BlockHash,
) -> Result<CLValue, CLValueError> {
    CLValue::from_t((timestamp_secs, Digest::from(parent_hash)))
}

/// Returns the Keccak-256 code hash for [`BEACON_ROOTS_CODE`].
pub fn beacon_roots_code_hash() -> evm::Hash {
    let digest = keccak256(BEACON_ROOTS_CODE);
    let mut hash = [0u8; evm::HASH_LENGTH];
    hash.copy_from_slice(digest.as_slice());
    evm::Hash::new(hash)
}

#[cfg(test)]
mod tests {
    use casper_types::{ByteCode, ByteCodeKind, Digest, StoredValue};

    use super::*;
    use crate::{
        global_state::state::{self, lmdb::LmdbGlobalStateView, StateProvider as _},
        tracking_copy::{TrackingCopy, TrackingCopyError, TrackingCopyExt},
    };

    fn tracking_copy(
        initial_data: impl IntoIterator<Item = (Key, StoredValue)>,
    ) -> (TrackingCopy<LmdbGlobalStateView>, impl Send) {
        let (global_state, root_hash, tempdir) =
            state::lmdb::make_temporary_global_state(initial_data);
        let reader = global_state
            .checkout(root_hash)
            .expect("checkout should not fail")
            .expect("root should exist");
        (TrackingCopy::new(reader, 5, false), tempdir)
    }

    #[test]
    fn constants_match_eip4788() {
        assert_eq!(
            BEACON_ROOTS_ADDRESS.to_hex_string(),
            "000f3df6d732807ef1319fb7b8bb8522d0beac02"
        );
        assert_eq!(HISTORY_BUFFER_LENGTH, 8_191);
        assert_eq!(BEACON_ROOTS_CODE.len(), 97);
        assert!(!beacon_roots_code_hash().is_zero());
    }

    #[test]
    fn slot_is_derived_from_timestamp() {
        assert_eq!(slot_for_timestamp(0), 0);
        assert_eq!(slot_for_timestamp(HISTORY_BUFFER_LENGTH - 1), 8_190);
        assert_eq!(slot_for_timestamp(HISTORY_BUFFER_LENGTH), 0);
    }

    #[test]
    fn tracking_copy_ext_reads_parent_hash_tuple() {
        let timestamp = 42;
        let parent_hash = BlockHash::new(Digest::from([7; Digest::LENGTH]));
        let value = parent_hash_value(timestamp, parent_hash).expect("tuple should encode");
        let (tracking_copy, _tempdir) =
            tracking_copy([(parent_hash_key(timestamp), StoredValue::CLValue(value))]);

        assert_eq!(
            tracking_copy
                .get_eip4788_parent_hash(timestamp)
                .expect("read should succeed"),
            Some((timestamp, parent_hash))
        );
    }

    #[test]
    fn tracking_copy_ext_sets_parent_hash_tuple() {
        let timestamp = 42;
        let parent_hash = BlockHash::new(Digest::from([7; Digest::LENGTH]));
        let (mut tracking_copy, _tempdir) = tracking_copy([]);

        tracking_copy
            .set_eip4788_parent_hash(timestamp, parent_hash)
            .expect("tuple should encode");

        let stored_value = tracking_copy
            .read(&parent_hash_key(timestamp))
            .expect("read should succeed")
            .expect("tuple should exist");
        let StoredValue::CLValue(cl_value) = stored_value else {
            panic!("EIP-4788 tuple should be stored as a CLValue");
        };
        assert_eq!(
            cl_value
                .into_t::<(u64, Digest)>()
                .expect("tuple should decode"),
            (timestamp, Digest::from(parent_hash))
        );
    }

    #[test]
    fn latest_entry_replaces_the_same_slot_after_a_full_ring() {
        let (mut tracking_copy, _tempdir) = tracking_copy([]);
        let replacement_timestamp = HISTORY_BUFFER_LENGTH + 1;
        let mut replacement_raw_hash = [0; Digest::LENGTH];
        replacement_raw_hash[..8].copy_from_slice(&replacement_timestamp.to_le_bytes());
        let replacement_hash = BlockHash::new(Digest::from(replacement_raw_hash));

        for timestamp in 1..=replacement_timestamp {
            let mut raw_hash = [0; Digest::LENGTH];
            raw_hash[..8].copy_from_slice(&timestamp.to_le_bytes());
            let parent_hash = BlockHash::new(Digest::from(raw_hash));
            tracking_copy
                .set_eip4788_parent_hash(timestamp, parent_hash)
                .expect("tuple should encode");
        }

        assert_eq!(
            tracking_copy
                .get_eip4788_parent_hash(1)
                .expect("read should succeed"),
            Some((replacement_timestamp, replacement_hash))
        );
    }

    #[test]
    fn returns_none_when_slot_is_absent() {
        let (tracking_copy, _tempdir) = tracking_copy([]);

        assert_eq!(
            tracking_copy
                .get_eip4788_parent_hash(42)
                .expect("read should succeed"),
            None
        );
    }

    #[test]
    fn rejects_unexpected_value_type() {
        let timestamp = 42;
        let (tracking_copy, _tempdir) = tracking_copy([(
            parent_hash_key(timestamp),
            StoredValue::ByteCode(ByteCode::new(ByteCodeKind::V1CasperWasm, vec![])),
        )]);

        assert!(matches!(
            tracking_copy.get_eip4788_parent_hash(timestamp),
            Err(TrackingCopyError::UnexpectedStoredValueVariant)
        ));
    }
}
