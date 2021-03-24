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
    types::GetStatusResult,
};

/// The status URL path.
pub const STATUS_API_PATH: &str = "status";

/// The metrics URL path.
pub const METRICS_API_PATH: &str = "metrics";

pub(super) fn create_status_filter<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    api_version: ProtocolVersion,
) -> BoxedFilter<(Response<Body>,)> {
    warp::get()
        .and(warp::path(STATUS_API_PATH))
        .and_then(move || {
            effect_builder
                .make_request(
                    |responder| RestRequest::GetStatus { responder },
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
                    |responder| RestRequest::GetMetrics { responder },
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
