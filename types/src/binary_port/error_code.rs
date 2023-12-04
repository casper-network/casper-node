//! Binary port error.

#[derive(Debug, Clone)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
#[repr(u8)]
pub enum ErrorCode {
    #[cfg_attr(feature = "std", error("request executed correctly"))]
    NoError = 0,
    #[cfg_attr(feature = "std", error("this function is disabled"))]
    FunctionIsDisabled = 1,
    //    #[cfg_attr(feature = "std", error("request cannot be decoded"))]
    //    InvalidRequest = 2, // TODO[RC]: handle this
    #[cfg_attr(feature = "std", error("data not found"))]
    NotFound = 3,
    #[cfg_attr(feature = "std", error("root not found"))]
    RootNotFound = 4,
    #[cfg_attr(feature = "std", error("invalid deploy item variant"))]
    InvalidDeployItemVariant = 5,
    #[cfg_attr(feature = "std", error("wasm preprocessing"))]
    WasmPreprocessing = 6,
    #[cfg_attr(feature = "std", error("invalid protocol version"))]
    InvalidProtocolVersion = 7,
    #[cfg_attr(feature = "std", error("invalid deploy"))]
    InvalidDeploy = 8,
    #[cfg_attr(feature = "std", error("internal error"))]
    InternalError = 9,
    #[cfg_attr(feature = "std", error("the query to global state failed to find a result"))]
    QueryFailed = 10,
    #[cfg_attr(feature = "std", error("the query to global state failed"))]
    QueryFailedToExecute = 11,
    //    #[cfg_attr(feature = "std", error("query global state failed"))]
    //    GetStateFailed = 11,
    //    #[cfg_attr(feature = "std", error("get all values failed"))]
    //    GetAllValuesFailed = 12,
    //    #[cfg_attr(feature = "std", error("get trie failed"))]
    //    GetTrieFailed = 13,
}
