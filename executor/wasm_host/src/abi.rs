use borsh::BorshSerialize;

#[derive(Copy, Clone, Debug, PartialEq, BorshSerialize)]
#[repr(C)]
pub(crate) struct ReadInfo {
    pub(crate) data_ptr: u32,
    /// Size in bytes.
    pub(crate) data_size: u32,
    /// Type UID of the data.
    ///
    /// This is a 64-bit unsigned integer that represents the type of the data.
    pub(crate) data_type_uid: u64,
}

#[cfg(test)]
unsafe impl safe_transmute::TriviallyTransmutable for ReadInfo {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, BorshSerialize)]

pub(crate) struct CreateResult {
    pub(crate) package_address: [u8; 32],
}

const _: () = assert!(
    std::mem::size_of::<CreateResult>() == 32,
    "CreateResult must be 32 bytes"
);

#[cfg(test)]
unsafe impl safe_transmute::TriviallyTransmutable for CreateResult {}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, BorshSerialize)]
pub(crate) struct CallResult {
    /// Result of the call.
    pub(crate) call_outcome: u32,
    /// Pointer to the data as returned from user's callback code.
    pub(crate) data_ptr: u32,
    /// */ Size in bytes.
    pub(crate) data_size: u32,
    /// Type UID of the data.
    ///
    /// This is a 64-bit unsigned integer that represents the type of the data.
    pub(crate) data_type: u64,
}

#[cfg(test)]
unsafe impl safe_transmute::TriviallyTransmutable for CallResult {}

#[derive(Clone, Copy, BorshSerialize, Debug, PartialEq)]
#[repr(C)]
pub struct EnvInfo {
    pub block_time: u64,
    pub transferred_value: u64,
    pub caller_addr: [u8; 32],
    pub caller_kind: u32,
    pub callee_addr: [u8; 32],
    pub callee_kind: u32,
}

#[cfg(test)]
unsafe impl safe_transmute::TriviallyTransmutable for EnvInfo {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_matches_transmute() {
        let env_info = EnvInfo {
            block_time: 1234567890,
            transferred_value: 1000,
            caller_addr: [1; 32],
            caller_kind: 2,
            callee_addr: [3; 32],
            callee_kind: 4,
        };

        let borsh_serialized = borsh::to_vec(&env_info).unwrap();
        let transmuted_object: EnvInfo = safe_transmute::transmute_one(&borsh_serialized).unwrap();

        assert_eq!(env_info, transmuted_object);

        let transmuted_bytes = safe_transmute::transmute_one_to_bytes(&env_info);
        assert_eq!(borsh_serialized, transmuted_bytes);
    }

    #[test]
    fn read_info_transmute() {
        let read_info = ReadInfo {
            data_ptr: 42,
            data_size: 100,
            data_type_uid: 12345678901234567890,
        };

        let transmuted_bytes = safe_transmute::transmute_one_to_bytes(&read_info);
        let transmuted_object: ReadInfo = safe_transmute::transmute_one(&transmuted_bytes).unwrap();

        assert_eq!(read_info, transmuted_object);
        assert_eq!(
            transmuted_bytes,
            safe_transmute::transmute_one_to_bytes(&transmuted_object)
        );
    }

    #[test]
    fn create_result_transmute() {
        let create_result = CreateResult {
            package_address: [1; 32],
        };
        let transmuted_bytes = safe_transmute::transmute_one_to_bytes(&create_result);
        let transmuted_object: CreateResult =
            safe_transmute::transmute_one(&transmuted_bytes).unwrap();
        assert_eq!(create_result, transmuted_object);

        assert_eq!(transmuted_bytes.len(), std::mem::size_of::<CreateResult>());
        assert_eq!(
            transmuted_bytes,
            safe_transmute::transmute_one_to_bytes(&transmuted_object)
        );
    }
}
