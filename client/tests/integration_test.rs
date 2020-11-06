use std::{convert::Infallible, sync::Mutex};

use futures::{channel::oneshot, future};
use hyper::{Body, Response, Server};
use lazy_static::lazy_static;
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

lazy_static! {
    /// This is used to serialize tests, as we take the lock at the start of a test and release when complete.
    /// Alternatively, setup your test to use a unique port.
    static ref TEST_LOCK_PORT: Mutex<u16> = Mutex::new(9090);
}

struct MockServerHandle {
    graceful_shutdown: oneshot::Sender<()>,
    server_joiner: JoinHandle<Result<(), hyper::error::Error>>,
}

impl MockServerHandle {
    async fn kill(self) {
        let _ = self.graceful_shutdown.send(());
        let _ = self.server_joiner.await;
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
    let server = builder.serve(make_svc);
    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });
    let server_joiner = tokio::spawn(server_with_shutdown);
    MockServerHandle {
        graceful_shutdown,
        server_joiner,
    }
}

macro_rules! assert_valid_response {
    ($response: expr) => {
        assert_eq!(
            $response
                .expect("parsed request values")
                .get_result()
                .map(|val| serde_json::from_value::<()>(val.clone()).unwrap()),
            Some(())
        );
    };
}

#[tokio::test(threaded_scheduler)] // needed to spawn the mock server in the background
async fn client_should_send_get_state_root_hash() {
    let node_port = 8999;
    let handle =
        spawn_mock_rpc_server::<GetStateRootHashParams>(node_port, GetStateRootHash::METHOD);

    let response = casper_client::get_state_root_hash(
        "1",
        &format!("http://localhost:{}", node_port),
        false,
        "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
    );
    assert_valid_response!(response);

    handle.kill().await;
}

#[tokio::test(threaded_scheduler)] // needed to spawn the mock server in the background
async fn client_should_send_get_deploy() {
    let node_port = 9001;
    let handle = spawn_mock_rpc_server::<GetDeployParams>(node_port, GetDeploy::METHOD);

    let response = casper_client::get_deploy(
        "1",
        &format!("http://localhost:{}", node_port),
        false,
        "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
    );
    assert_valid_response!(response);

    handle.kill().await;
}

#[tokio::test(threaded_scheduler)]
async fn client_should_send_put_deploy() {
    let node_port = 9002;
    let handle = spawn_mock_rpc_server::<PutDeployParams>(node_port, PutDeploy::METHOD);

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
    assert_valid_response!(response);
    handle.kill().await;
}

#[tokio::test(threaded_scheduler)]
#[should_panic(
    expected = "InvalidArgument(\"Invalid arguments to get_transfer_target - must provide either a target account or purse.\")"
)]
async fn client_transfer_with_target_purse_and_target_account_should_fail() {
    casper_client::transfer(
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
    )
    .unwrap();
}
