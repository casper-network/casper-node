use std::{fs, str};

use clap::{App, Arg, ArgMatches, SubCommand};

use casper_node::{
    crypto::asymmetric_key::PublicKey,
    rpcs::{
        state::{GetItem, GetItemParams},
        RpcWithParams,
    },
};
use casper_types::Key;

use crate::{command::ClientCommand, common, RpcClient};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    StateRootHash,
    Key,
    Path,
}

/// Handles providing the arg for and retrieval of the key.
mod key {
    use super::*;

    const ARG_NAME: &str = "key";
    const ARG_SHORT: &str = "k";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING or PATH";
    const ARG_HELP: &str =
        "The base key for the query.  This must be a properly formatted public key, account hash, \
        contract address hash or URef.  The format for each respectively is \"<HEX STRING>\", \
        \"account-hash-<HEX STRING>\", \"hash-<HEX STRING>\" and \
        \"uref-<HEX STRING>-<THREE DIGIT INTEGER>\".  The public key may instead be read in from a \
        file, in which case enter the path to the file as the --key argument.  The file should be \
        one of the two public key files generated via the `keygen` subcommand; \"public_key_hex\" \
        or \"public_key.pem\"";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Key as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> String {
        let value = matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME));

        // Try to parse as a `Key` first.
        if Key::from_formatted_str(value).is_ok() {
            return value.to_string();
        }

        // Try to parse from a hex-encoded `PublicKey`, a pem-encoded file then a hex-encoded file.
        let public_key = if let Ok(public_key) = PublicKey::from_hex(value) {
            public_key
        } else if let Ok(public_key) = PublicKey::from_file(value) {
            public_key
        } else {
            let contents = fs::read(value).unwrap_or_else(|_| {
                panic!(
                    "failed to parse '{}' as a public key (as a hex string, hex file or pem file), \
                    account hash, contract address hash or URef",
                    value
                )
            });
            PublicKey::from_hex(contents).unwrap_or_else(|error| {
                panic!(
                    "failed to parse '{}' as a hex-encoded public key file: {}",
                    value, error
                )
            })
        };

        // Return the public key as an account hash.
        public_key.to_account_hash().to_formatted_string()
    }
}

/// Handles providing the arg for and retrieval of the key.
mod path {
    use super::*;

    const ARG_NAME: &str = "query-path";
    const ARG_SHORT: &str = "q";
    const ARG_VALUE_NAME: &str = "PATH/FROM/BASE/KEY";
    const ARG_HELP: &str = "The path from the base key for the query";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(false)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::Path as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> Vec<String> {
        match matches.value_of(ARG_NAME) {
            Some("") | None => return vec![],
            Some(path) => path.split('/').map(ToString::to_string).collect(),
        }
    }
}

impl RpcClient for GetItem {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetItem {
    const NAME: &'static str = "query-state";
    const ABOUT: &'static str = "Retrieves a stored value from global state";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(common::state_root_hash::arg(
                DisplayOrder::StateRootHash as usize,
            ))
            .arg(key::arg())
            .arg(path::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let verbose = common::verbose::get(matches);
        let node_address = common::node_address::get(matches);
        let rpc_id = common::rpc_id::get(matches);
        let state_root_hash = common::state_root_hash::get(matches);
        let key = key::get(matches);
        let path = path::get(matches);

        let params = GetItemParams {
            state_root_hash: state_root_hash.to_owned(),
            key: key.to_owned(),
            path: path.to_owned(),
        };

        let response = Self::request_with_map_params(verbose, &node_address, rpc_id, params);
        let key = Key::from_formatted_str(&key).expect("key should convert to key");
        match merkle_proofs::validate_response(&response, &state_root_hash, &key, &path) {
            Ok(_) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&response).expect("should encode to JSON")
                );
            }
            Err(error) => panic!("{:?}", error),
        }
    }
}

mod merkle_proofs {
    use jsonrpc_lite::JsonRpc;

    use thiserror::Error;

    use casper_execution_engine::{
        core, core::ValidationError, shared::stored_value::StoredValue,
        storage::trie::merkle_proof::TrieMerkleProof,
    };
    use casper_node::{crypto::hash::Digest, types::json_compatibility};
    use casper_types::{bytesrepr, Key};
    use std::convert::TryFrom;

    const GET_ITEM_RESULT_STORED_VALUE: &str = "stored_value";
    const GET_ITEM_RESULT_MERKLE_PROOF: &str = "merkle_proof";

    /// Error that can be returned by validate_response.
    #[derive(Error, Debug)]
    pub enum ValidateResponseError {
        /// Failed to marshall value
        #[error("Failed to marshall value {0}")]
        BytesRepr(bytesrepr::Error),

        /// Error from serde.
        #[error(transparent)]
        Serde(#[from] serde_json::Error),

        /// [`validate_response`] failed to parse JSON
        #[error("validate_response failed to parse")]
        ValidateResponseFailedToParse,

        /// [`validate_response`] failed to validate
        #[error(transparent)]
        ValidationError(#[from] ValidationError),

        /// Serialized value not contained in proof
        #[error("serialized value not contained in proof")]
        SerializedValueNotContainedInProof,
    }

    impl From<bytesrepr::Error> for ValidateResponseError {
        fn from(e: bytesrepr::Error) -> Self {
            ValidateResponseError::BytesRepr(e)
        }
    }

    pub fn validate_response(
        response: &JsonRpc,
        state_root_hash: &Digest,
        key: &Key,
        path: &[String],
    ) -> Result<(), ValidateResponseError> {
        let value = response
            .get_result()
            .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;

        let object = value
            .as_object()
            .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;

        let proofs: Vec<TrieMerkleProof<Key, StoredValue>> = {
            let proof = object
                .get(GET_ITEM_RESULT_MERKLE_PROOF)
                .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;
            let proof_str = proof
                .as_str()
                .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;
            let proof_bytes = hex::decode(proof_str)
                .map_err(|_| ValidateResponseError::ValidateResponseFailedToParse)?;
            bytesrepr::deserialize(proof_bytes)?
        };

        let proof_value: &StoredValue = {
            let last_proof = proofs
                .last()
                .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;
            last_proof.value()
        };

        // Here we need to validate that JSON `stored_value` is contained in the proof.
        //
        // Possible to deserialize that field into a `StoredValue` and pass below to
        // `validate_query_proof` instead of using this approach?
        {
            let value: json_compatibility::StoredValue = {
                let value = object
                    .get(GET_ITEM_RESULT_STORED_VALUE)
                    .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;
                serde_json::from_value(value.to_owned())?
            };
            match json_compatibility::StoredValue::try_from(proof_value) {
                Ok(json_proof_value) if json_proof_value == value => (),
                _ => return Err(ValidateResponseError::SerializedValueNotContainedInProof),
            }
        }

        core::validate_query_proof(
            &state_root_hash.to_owned().into(),
            &proofs,
            key,
            path,
            proof_value,
        )
        .map_err(Into::into)
    }
}
