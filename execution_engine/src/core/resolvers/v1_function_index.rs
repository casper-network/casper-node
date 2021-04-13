use std::convert::TryFrom;

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::{FromPrimitive, ToPrimitive};

#[derive(Debug, PartialEq, FromPrimitive, ToPrimitive, Clone, Copy)]
#[repr(usize)]
pub enum FunctionIndex {
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
    RecordTransfer,
    RecordEraInfo,
    Delete,
}

impl Into<usize> for FunctionIndex {
    fn into(self) -> usize {
        // NOTE: This can't fail as `FunctionIndex` is represented by usize,
        // so this serves mostly as a syntax sugar.
        self.to_usize().unwrap()
    }
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
    use std::convert::TryFrom;

    #[test]
    fn primitive_to_enum() {
        FunctionIndex::try_from(19).expect("Unable to create enum from number");
    }

    #[test]
    fn enum_to_primitive() {
        let element = FunctionIndex::UpdateAssociatedKeyFuncIndex;
        let _primitive: usize = element.into();
    }

    #[test]
    fn invalid_index() {
        assert!(FunctionIndex::try_from(123_456_789usize).is_err());
    }
}
