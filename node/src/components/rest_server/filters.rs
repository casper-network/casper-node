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
    rpcs::info::{GetValidatorChangesResult, JsonEraChange, JsonValidatorInfo},
    types::GetStatusResult,
};

/// The status URL path.
pub const STATUS_API_PATH: &str = "status";

/// The metrics URL path.
pub const METRICS_API_PATH: &str = "metrics";

/// The OpenRPC scehma URL path.
pub const JSON_RPC_SCHEMA_API_PATH: &str = "rpc-schema";

/// The validator information URL path.
pub const VALIDATOR_CHANGES_API_PATH: &str = "validator-changes";

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

pub(super) fn create_validator_info_filter<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
) -> BoxedFilter<(Response<Body>,)> {
    warp::get()
        .and(warp::path(VALIDATOR_CHANGES_API_PATH))
        .and_then(move || {
            effect_builder
                .get_consensus_validator_changes()
                .map(move |validator_info| {
                    let json_validator_info = validator_info
                        .into_iter()
                        .map(|(public_key, era_changes)| {
                            let era_changes = era_changes
                                .into_iter()
                                .map(|(era_id, validator_change)| {
                                    JsonEraChange::new(era_id, validator_change)
                                })
                                .collect();
                            JsonValidatorInfo::new(public_key, era_changes)
                        })
                        .collect();
                    let result = GetValidatorChangesResult {
                        api_version,
                        changes_to_validators: json_validator_info,
                    };
                    Ok::<_, Rejection>(reply::json(&result).into_response())
                })
        })
        .boxed()
}
