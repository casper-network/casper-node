//! Binary port error.

#[derive(Debug, Clone)]
#[cfg_attr(feature = "std", derive(thiserror::Error))]
#[repr(u8)]
pub enum Error {
    #[cfg_attr(feature = "std", error("request executed correctly"))]
    NoError = 0,
    #[cfg_attr(feature = "std", error("data not found"))]
    NotFound = 1,
    #[cfg_attr(feature = "std", error("transaction not accepted"))]
    TransactionNotAccepted = 2,
    #[cfg_attr(feature = "std", error("root not found"))]
    RootNotFound = 3,
    #[cfg_attr(feature = "std", error("invalid deploy item variant"))]
    InvalidDeployItemVariant = 4,
    #[cfg_attr(feature = "std", error("wasm preprocessing"))]
    WasmPreprocessing = 5,
    #[cfg_attr(feature = "std", error("invalid protocol version"))]
    InvalidProtocolVersion = 6,
    #[cfg_attr(feature = "std", error("invalid deploy"))]
    InvalidDeploy = 7,
    #[cfg_attr(feature = "std", error("internal error"))]
    InternalError = 8,
    #[cfg_attr(feature = "std", error("query global state failed"))]
    GetStateFailed = 9,
    #[cfg_attr(feature = "std", error("this function is disabled"))]
    FunctionIsDisabled = 10,
    #[cfg_attr(feature = "std", error("get all values failed"))]
    GetAllValuesFailed = 11,
}
