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

use super::{GetChainspecResult, GetValidatorChangesResult, ReactorEventT};
use crate::{
    effect::{requests::RestRequest, EffectBuilder},
    reactor::QueueKind,
    types::{ChainspecStatus, GetStatusResult},
};

/// The status URL path.
pub const STATUS_API_PATH: &str = "status";

/// The metrics URL path.
pub const METRICS_API_PATH: &str = "metrics";

/// The validator information URL path.
pub const VALIDATOR_CHANGES_API_PATH: &str = "validator-changes";

/// The chainspec file URL path.
pub const CHAINSPEC_API_PATH: &str = "chainspec";

/// The running and installed future chainspec source documents URL path.
pub const CHAINSPECS_API_PATH: &str = "chainspecs";

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

pub(super) fn create_chainspecs_filter<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
) -> BoxedFilter<(Response<Body>,)> {
    warp::get()
        .and(warp::path(CHAINSPECS_API_PATH))
        .and(warp::path::end())
        .and_then(move || {
            effect_builder.get_chainspecs().map(|chainspecs| {
                let status = if chainspecs.is_ok() {
                    StatusCode::OK
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                let result = ChainspecStatus::from(chainspecs);
                Ok::<_, Rejection>(reply::with_status(reply::json(&result), status).into_response())
            })
        })
        .boxed()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc, time::Duration};

    use casper_types::ChainspecRawBytes;

    use super::*;
    use crate::{
        effect::requests::UpgradeWatcherRequest,
        reactor::{main_reactor::MainEvent, EventQueueHandle, Scheduler},
        types::Chainspecs,
        utils,
    };

    async fn chainspecs_response(metadata: Result<Chainspecs, String>) -> Response<bytes::Bytes> {
        let scheduler = utils::leak(Scheduler::<MainEvent>::new(QueueKind::weights(), None));
        let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(scheduler));
        let filter = create_chainspecs_filter(effect_builder);
        let request = warp::test::request().path("/chainspecs").reply(&filter);
        let respond = async {
            let ((_, event), _) = scheduler.pop().await;
            match event {
                MainEvent::UpgradeWatcherRequest(UpgradeWatcherRequest::Chainspecs(responder)) => {
                    responder.respond(metadata).await
                }
                other => panic!("unexpected chainspecs request: {other:?}"),
            }
        };
        let (response, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            futures::join!(request, respond)
        })
        .await
        .expect("chainspecs request should complete");
        response
    }

    #[tokio::test]
    async fn chainspecs_returns_complete_source_documents() {
        let source = Arc::new(ChainspecRawBytes::new(
            b"# chainspec\n".to_vec().into(),
            Some(b"# accounts\n".to_vec().into()),
            Some(b"# state\n".to_vec().into()),
        ));
        let metadata = Chainspecs {
            current: source.clone(),
            future: BTreeMap::from([
                (ProtocolVersion::from_parts(2, 2, 0), source.clone()),
                (ProtocolVersion::from_parts(2, 3, 0), source),
            ]),
        };
        let response = chainspecs_response(Ok(metadata.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(
            serde_json::from_slice::<ChainspecStatus>(response.body()).unwrap(),
            ChainspecStatus::Available(metadata)
        );
    }

    #[tokio::test]
    async fn chainspecs_returns_empty_future_map_when_no_upgrade_is_installed() {
        let metadata = Chainspecs {
            current: Arc::new(ChainspecRawBytes::new(Vec::new().into(), None, None)),
            future: BTreeMap::new(),
        };
        let response = chainspecs_response(Ok(metadata)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
        assert_eq!(json["future"], serde_json::json!({}));
    }

    #[tokio::test]
    async fn chainspecs_returns_http_error_when_discovery_is_unavailable() {
        let response = chainspecs_response(Err("unreadable installed chainspec".to_string())).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(response.body()).unwrap(),
            serde_json::json!({"error": "unreadable installed chainspec"})
        );
    }
}
