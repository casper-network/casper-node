//! WASM host function resolver for protocol version 1.x.x.
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

/// Enum representing unique IDs of host functions supported in major version 1.
#[derive(Debug, PartialEq, FromPrimitive, Clone, Copy)]
#[repr(usize)]
pub(crate) enum FunctionIndex {
    WriteFuncIndex,
    ReadFuncIndex,
    AddFuncIndex,
    NewFuncIndex,
    RetFuncIndex,
    CallContractFuncIndex,
    GetKeyFuncIndex,
    GasFuncIndex,
    HasKeyFuncIndex,
    PutKeyFuncIndex,
    IsValidURefFnIndex,
    RevertFuncIndex,
    AddAssociatedKeyFuncIndex,
    RemoveAssociatedKeyFuncIndex,
    UpdateAssociatedKeyFuncIndex,
    SetActionThresholdFuncIndex,
    LoadNamedKeysFuncIndex,
    RemoveKeyFuncIndex,
    GetCallerIndex,
    GetBlocktimeIndex,
    CreatePurseIndex,
    TransferToAccountIndex,
    TransferFromPurseToAccountIndex,
    TransferFromPurseToPurseIndex,
    GetBalanceIndex,
    GetPhaseIndex,
    GetSystemContractIndex,
    GetMainPurseIndex,
    ReadHostBufferIndex,
    CreateContractPackageAtHash,
    AddContractVersion,
    AddContractVersionWithMessageTopics,
    AddPackageVersionWithMessageTopics,
    DisableContractVersion,
    CallVersionedContract,
    CreateContractUserGroup,
    #[cfg(feature = "test-support")]
    PrintIndex,
    GetRuntimeArgsizeIndex,
    GetRuntimeArgIndex,
    RemoveContractUserGroupIndex,
    ExtendContractUserGroupURefsIndex,
    RemoveContractUserGroupURefsIndex,
    Blake2b,
    NewDictionaryFuncIndex,
    DictionaryGetFuncIndex,
    DictionaryPutFuncIndex,
    LoadCallStack,
    LoadAuthorizationKeys,
    RandomBytes,
    DictionaryReadFuncIndex,
    EnableContractVersion,
    ManageMessageTopic,
    EmitMessage,
    LoadCallerInformation,
    GetBlockInfoIndex,
    GenericHash,
    RecoverSecp256k1,
    VerifySignature,
    CallPackageVersion,
}

impl TryFrom<usize> for FunctionIndex {
    type Error = &'static str;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        FromPrimitive::from_usize(value).ok_or("Invalid function index")
    }
}

#[cfg(test)]
mod tests {
    use super::FunctionIndex;

    #[test]
    fn primitive_to_enum() {
        FunctionIndex::try_from(19).expect("Unable to create enum from number");
    }

    #[test]
    fn enum_to_primitive() {
        let element = FunctionIndex::UpdateAssociatedKeyFuncIndex;
        let _primitive: usize = element as usize;
    }

    #[test]
    fn invalid_index() {
        assert!(FunctionIndex::try_from(123_456_789usize).is_err());
    }
}
