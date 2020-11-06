use std::convert::Infallible;

use futures::{channel::oneshot, future};
use hyper::{Body, Response, Server};
use serde::Deserialize;
use tokio::task::JoinHandle;
use warp::{Filter, Rejection};
use warp_json_rpc::Builder;

use casper_client::{DeployStrParams, PaymentStrParams, SessionStrParams};
use casper_node::rpcs::{
    account::{PutDeploy, PutDeployParams},
    chain::{GetStateRootHash, GetStateRootHashParams},
    info::{GetDeploy, GetDeployParams},
    RpcWithOptionalParams, RpcWithParams,
};

fn test_filter<P>(
    method: &'static str,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Copy
where
    for<'de> P: Deserialize<'de> + Send,
{
    warp_json_rpc::filters::json_rpc()
        .and(warp_json_rpc::filters::method(method))
        .and(warp_json_rpc::filters::params::<P>())
        .map(|builder: Builder, _params: P| builder.success(()).unwrap())
}

struct MockServerHandle {
    graceful_shutdown: Option<oneshot::Sender<()>>,
    server_joiner: Option<JoinHandle<Result<(), hyper::error::Error>>>,
}
impl Drop for MockServerHandle {
    fn drop(&mut self) {
        let _ = self.graceful_shutdown.take().unwrap().send(());
        let joiner = self.server_joiner.take().unwrap();
        futures::executor::block_on(async {
            let _ = joiner.await;
        });
    }
}

/// Will spawn a server on localhost at `port` and respond to JSON-RPC requests that successfully
/// deserialize as `P`.
fn spawn_mock_rpc_server<P>(port: u16, method: &'static str) -> MockServerHandle
where
    P: 'static,
    for<'de> P: Deserialize<'de> + Send,
{
    let service = warp_json_rpc::service(test_filter::<P>(method));
    let builder = Server::try_bind(&([127, 0, 0, 1], port).into()).unwrap();
    let make_svc =
        hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));
    let (graceful_shutdown, shutdown_receiver) = oneshot::channel::<()>();
    let graceful_shutdown = Some(graceful_shutdown);
    let server = builder.serve(make_svc);
    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });
    let server_joiner = tokio::spawn(server_with_shutdown);
    let server_joiner = Some(server_joiner);
    MockServerHandle {
        graceful_shutdown,
        server_joiner,
    }
}

#[tokio::test(threaded_scheduler)]
async fn client_should_send_get_state_root_hash() {
    let node_port = 8999;
    let _ = spawn_mock_rpc_server::<GetStateRootHashParams>(node_port, GetStateRootHash::METHOD);

    let response = casper_client::get_state_root_hash(
        "1",
        &format!("http://localhost:{}", node_port),
        false,
        "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
    );
    assert_eq!(
        response
            .expect("parsed request values")
            .get_result()
            .map(|val| serde_json::from_value::<()>(val.clone()).unwrap()),
        Some(())
    );
}

#[tokio::test(threaded_scheduler)]
async fn client_should_send_get_deploy() {
    let node_port = 9001;
    let _ = spawn_mock_rpc_server::<GetDeployParams>(node_port, GetDeploy::METHOD);

    let response = casper_client::get_deploy(
        "1",
        &format!("http://localhost:{}", node_port),
        false,
        "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
    );
    assert_eq!(
        response
            .expect("parsed request values")
            .get_result()
            .map(|val| serde_json::from_value::<()>(val.clone()).unwrap()),
        Some(())
    );
}

#[tokio::test(threaded_scheduler)]
async fn client_should_send_put_deploy() {
    let node_port = 9002;
    let _ = spawn_mock_rpc_server::<PutDeployParams>(node_port, PutDeploy::METHOD);

    let response = casper_client::put_deploy(
        "1",
        &format!("http://localhost:{}", node_port),
        false,
        DeployStrParams {
            secret_key: "../resources/local/secret_keys/node-1.pem",
            ttl: "10s",
            chain_name: "casper-test-chain-name-1",
            gas_price: "1",
            ..Default::default()
        },
        SessionStrParams::with_path(
            "../target/wasm32-unknown-unknown/release/standard_payment.wasm",
            vec!["name_01:bool='false'", "name_02:i32='42'"],
            "",
        ),
        PaymentStrParams::with_amount("100"),
    );
    assert_eq!(
        response
            .expect("parsed request values")
            .get_result()
            .map(|val| serde_json::from_value::<()>(val.clone()).unwrap()),
        Some(())
    );
}

#[tokio::test(threaded_scheduler)]
async fn client_transfer_with_target_purse_and_target_account_should_fail() {
    let result = casper_client::transfer(
        "1",
        &format!("http://localhost:{}", 12345),
        false,
        "12345",
        "",
        "some-target-purse",
        "some-target-account",
        DeployStrParams {
            secret_key: "../resources/local/secret_keys/node-1.pem",
            ttl: "10s",
            chain_name: "casper-test-chain-name-1",
            gas_price: "1",
            ..Default::default()
        },
        PaymentStrParams::with_amount("100"),
    );
    assert!(result.is_err());
}
