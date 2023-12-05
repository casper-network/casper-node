use serde::{Deserialize, Serialize};

use casper_json_rpc::ErrorCodeT;

/// The various codes which can be returned in the JSON-RPC Response's error object.
///
/// **NOTE:** These values will be changed to lie outside the restricted range as defined in the
/// JSON-RPC spec as of casper-node v2.0.0.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug)]
#[repr(i64)]
pub enum ErrorCode {
    /// The requested Deploy was not found.
    NoSuchDeploy = -32000,
    /// The requested Block was not found.
    NoSuchBlock = -32001,
    /// Parsing the Key for a query failed.
    FailedToParseQueryKey = -32002,
    /// The query failed to find a result.
    QueryFailed = -32003,
    /// Executing the query failed.
    QueryFailedToExecute = -32004,
    /// Parsing the URef while getting a balance failed.
    FailedToParseGetBalanceURef = -32005,
    /// Failed to get the requested balance.
    FailedToGetBalance = -32006,
    /// Executing the query to retrieve the balance failed.
    GetBalanceFailedToExecute = -32007,
    /// The given Deploy cannot be executed as it is invalid.
    InvalidDeploy = -32008,
    /// The given account was not found.
    NoSuchAccount = -32009,
    /// Failed to get the requested dictionary URef.
    FailedToGetDictionaryURef = -32010,
    /// Failed to get the requested dictionary trie.
    FailedToGetTrie = -32011,
    /// The requested state root hash was not found.
    NoSuchStateRoot = -32012,
    /// The main purse for a given account hash does not exist.
    NoSuchMainPurse = -32013,
    /// The requested Transaction was not found.
    NoSuchTransaction = -32014,
    /// Variant mismatch.
    VariantMismatch = -32015,
    /// The given Transaction cannot be executed as it is invalid.
    InvalidTransaction = -32016,
    /// The given Block could not be verified.
    InvalidBlock = -32017,
    /// Failed during a node request.
    NodeRequestFailed = -32018,
    /// Auction state could not be parsed.
    InvalidAuctionState = -32019,
    /// The request could not be satisfied because an underlying function is disabled.
    FunctionIsDisabled = -32020,
}

impl From<ErrorCode> for (i64, &'static str) {
    fn from(error_code: ErrorCode) -> Self {
        match error_code {
            ErrorCode::NoSuchDeploy => (error_code as i64, "No such deploy"),
            ErrorCode::NoSuchBlock => (error_code as i64, "No such block"),
            ErrorCode::FailedToParseQueryKey => (error_code as i64, "Failed to parse query key"),
            ErrorCode::QueryFailed => (error_code as i64, "Query failed"),
            ErrorCode::QueryFailedToExecute => (error_code as i64, "Query failed to execute"),
            ErrorCode::FailedToParseGetBalanceURef => {
                (error_code as i64, "Failed to parse get-balance URef")
            }
            ErrorCode::FailedToGetBalance => (error_code as i64, "Failed to get balance"),
            ErrorCode::GetBalanceFailedToExecute => {
                (error_code as i64, "get-balance failed to execute")
            }
            ErrorCode::InvalidDeploy => (error_code as i64, "Invalid Deploy"),
            ErrorCode::NoSuchAccount => (error_code as i64, "No such account"),
            ErrorCode::FailedToGetDictionaryURef => {
                (error_code as i64, "Failed to get dictionary URef")
            }
            ErrorCode::FailedToGetTrie => (error_code as i64, "Failed to get trie"),
            ErrorCode::NoSuchStateRoot => (error_code as i64, "No such state root"),
            ErrorCode::NoSuchMainPurse => (error_code as i64, "Failed to get main purse"),
            ErrorCode::NoSuchTransaction => (error_code as i64, "No such transaction"),
            ErrorCode::VariantMismatch => (error_code as i64, "Variant mismatch internal error"),
            ErrorCode::InvalidTransaction => (error_code as i64, "Invalid transaction"),
            ErrorCode::InvalidBlock => (error_code as i64, "Invalid block"),
            ErrorCode::NodeRequestFailed => (error_code as i64, "Node request failure"),
            ErrorCode::InvalidAuctionState => (error_code as i64, "Invalid auction state"),
            ErrorCode::FunctionIsDisabled => (
                error_code as i64,
                "Function needed to execute this request is disabled",
            ),
        }
    }
}

impl ErrorCodeT for ErrorCode {}
