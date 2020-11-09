use std::{convert::Infallible, net::SocketAddr};

use futures::{channel::oneshot, future};
use hyper::{Body, Response, Server};
use serde::Deserialize;
use tokio::task::JoinHandle;
use warp::{Filter, Rejection};
use warp_json_rpc::Builder;

use casper_client::{DeployStrParams, Error, PaymentStrParams, SessionStrParams};
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
    address: SocketAddr,
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

/// Will spawn a server on localhost and respond to JSON-RPC requests that successfully
/// deserialize as `P`.
fn spawn_mock_rpc_server<P>(method: &'static str) -> MockServerHandle
where
    P: 'static,
    for<'de> P: Deserialize<'de> + Send,
{
    let service = warp_json_rpc::service(test_filter::<P>(method));
    let builder = Server::try_bind(&([127, 0, 0, 1], 0).into()).unwrap();
    let make_svc =
        hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));
    let (graceful_shutdown, shutdown_receiver) = oneshot::channel::<()>();
    let graceful_shutdown = Some(graceful_shutdown);
    let server = builder.serve(make_svc);
    let address = server.local_addr();
    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });
    let server_joiner = tokio::spawn(server_with_shutdown);
    let server_joiner = Some(server_joiner);
    MockServerHandle {
        graceful_shutdown,
        server_joiner,
        address,
    }
}

#[tokio::test(threaded_scheduler)]
async fn client_should_send_get_state_root_hash() {
    let server_handle = spawn_mock_rpc_server::<GetStateRootHashParams>(GetStateRootHash::METHOD);

    let response = casper_client::get_state_root_hash(
        "1",
        &format!("http://{}", server_handle.address),
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
    let server_handle = spawn_mock_rpc_server::<GetDeployParams>(GetDeploy::METHOD);

    let response = casper_client::get_deploy(
        "1",
        &format!("http://{}", server_handle.address),
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
    let server_handle = spawn_mock_rpc_server::<PutDeployParams>(PutDeploy::METHOD);

    let response = casper_client::put_deploy(
        "1",
        &format!("http://{}", server_handle.address),
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
        "http://localhost:12345",
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
    let expected_arg_name = "target_account | target_purse";
    let expected_msg =
        "Invalid arguments to get_transfer_target - must provide either a target account or purse.";
    match result {
        Err(Error::InvalidArgument(arg_name, msg))
            if (arg_name, msg.as_str()) == (expected_arg_name, expected_msg) => {}
        _ => panic!(
            "Expected {:?}, but got {:?}",
            Error::InvalidArgument(expected_arg_name, expected_msg.to_string()),
            result
        ),
    }
}
