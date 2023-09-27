//! RPCs related to accounts.

use std::str;

use async_trait::async_trait;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::debug;

use casper_types::{Deploy, DeployHash, ProtocolVersion, Transaction};

use super::{
    docs::{DocExample, DOCS_EXAMPLE_PROTOCOL_VERSION},
    Error, ReactorEventT, RpcWithParams,
};
use crate::{components::rpc_server::rpcs::ErrorCode, effect::EffectBuilder};

static PUT_DEPLOY_PARAMS: Lazy<PutDeployParams> = Lazy::new(|| PutDeployParams {
    deploy: Deploy::doc_example().clone(),
});
static PUT_DEPLOY_RESULT: Lazy<PutDeployResult> = Lazy::new(|| PutDeployResult {
    api_version: DOCS_EXAMPLE_PROTOCOL_VERSION,
    deploy_hash: *Deploy::doc_example().hash(),
});

/// Params for "account_put_deploy" RPC request.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutDeployParams {
    /// The `Deploy`.
    pub deploy: Deploy,
}

impl DocExample for PutDeployParams {
    fn doc_example() -> &'static Self {
        &PUT_DEPLOY_PARAMS
    }
}

/// Result for "account_put_deploy" RPC response.
#[derive(PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutDeployResult {
    /// The RPC API version.
    #[schemars(with = "String")]
    pub api_version: ProtocolVersion,
    /// The deploy hash.
    pub deploy_hash: DeployHash,
}

impl DocExample for PutDeployResult {
    fn doc_example() -> &'static Self {
        &PUT_DEPLOY_RESULT
    }
}

/// "account_put_deploy" RPC
pub struct PutDeploy {}

#[async_trait]
impl RpcWithParams for PutDeploy {
    const METHOD: &'static str = "account_put_deploy";
    type RequestParams = PutDeployParams;
    type ResponseResult = PutDeployResult;

    async fn do_handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        api_version: ProtocolVersion,
        params: Self::RequestParams,
    ) -> Result<Self::ResponseResult, Error> {
        let deploy_hash = *params.deploy.hash();

        let accept_transaction_result = effect_builder
            .try_accept_transaction(Transaction::from(params.deploy), None)
            .await;

        match accept_transaction_result {
            Ok(_) => {
                debug!(%deploy_hash, "deploy was stored");
                let result = Self::ResponseResult {
                    api_version,
                    deploy_hash,
                };
                Ok(result)
            }
            Err(error) => {
                debug!(
                    %deploy_hash,
                    %error,
                    "the deploy submitted by the client was invalid",
                );
                Err(Error::new(ErrorCode::InvalidDeploy, error.to_string()))
            }
        }
    }
}
