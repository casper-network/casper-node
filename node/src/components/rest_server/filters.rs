use futures::FutureExt;
use http::Response;
use hyper::Body;
use tracing::warn;
use warp::{
    filters::BoxedFilter,
    http::StatusCode,
    reject::Rejection,
    reply::{self, Reply},
    Filter,
};

use casper_types::ProtocolVersion;

use super::ReactorEventT;
use crate::{
    effect::{requests::RestRequest, EffectBuilder},
    reactor::QueueKind,
    rpcs::info::{GetChainspecResult, GetValidatorChangesResult},
    types::GetStatusResult,
};

/// The status URL path.
pub const STATUS_API_PATH: &str = "status";

/// The metrics URL path.
pub const METRICS_API_PATH: &str = "metrics";

/// The OpenRPC schema URL path.
pub const JSON_RPC_SCHEMA_API_PATH: &str = "rpc-schema";

/// The validator information URL path.
pub const VALIDATOR_CHANGES_API_PATH: &str = "validator-changes";

/// The chainspec file URL path.
pub const CHAINSPEC_API_PATH: &str = "chainspec";

pub(super) fn create_status_filter<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
) -> BoxedFilter<(Response<Body>,)> {
    warp::get()
        .and(warp::path(STATUS_API_PATH))
        .and_then(move || {
            effect_builder
                .make_request(
                    |responder| RestRequest::Status { responder },
                    QueueKind::Api,
                )
                .map(move |status_feed| {
                    let body = GetStatusResult::new(status_feed, api_version);
                    Ok::<_, Rejection>(reply::json(&body).into_response())
                })
        })
        .boxed()
}

pub(super) fn create_metrics_filter<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
) -> BoxedFilter<(Response<Body>,)> {
    warp::get()
        .and(warp::path(METRICS_API_PATH))
        .and_then(move || {
            effect_builder
                .make_request(
                    |responder| RestRequest::Metrics { responder },
                    QueueKind::Api,
                )
                .map(|maybe_metrics| match maybe_metrics {
                    Some(metrics) => Ok::<_, Rejection>(
                        reply::with_status(metrics, StatusCode::OK).into_response(),
                    ),
                    None => {
                        warn!("metrics not available");
                        Ok(reply::with_status(
                            "metrics not available",
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                        .into_response())
                    }
                })
        })
        .boxed()
}

pub(super) fn create_rpc_schema_filter<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
) -> BoxedFilter<(Response<Body>,)> {
    warp::get()
        .and(warp::path(JSON_RPC_SCHEMA_API_PATH))
        .and_then(move || {
            effect_builder
                .make_request(
                    |responder| RestRequest::RpcSchema { responder },
                    QueueKind::Api,
                )
                .map(move |open_rpc_schema| {
                    Ok::<_, Rejection>(reply::json(&open_rpc_schema).into_response())
                })
        })
        .boxed()
}

pub(super) fn create_validator_changes_filter<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
) -> BoxedFilter<(Response<Body>,)> {
    warp::get()
        .and(warp::path(VALIDATOR_CHANGES_API_PATH))
        .and_then(move || {
            effect_builder
                .get_consensus_validator_changes()
                .map(move |changes| {
                    let result = GetValidatorChangesResult::new(api_version, changes);
                    Ok::<_, Rejection>(reply::json(&result).into_response())
                })
        })
        .boxed()
}

pub(super) fn create_chainspec_filter<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
) -> BoxedFilter<(Response<Body>,)> {
    warp::get()
        .and(warp::path(CHAINSPEC_API_PATH))
        .and_then(move || {
            effect_builder
                .get_chainspec_raw_bytes()
                .map(move |chainspec_bytes| {
                    let result = GetChainspecResult::new(api_version, (*chainspec_bytes).clone());
                    Ok::<_, Rejection>(reply::json(&result).into_response())
                })
        })
        .boxed()
}
