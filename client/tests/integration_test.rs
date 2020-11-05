use std::{convert::Infallible, sync::Mutex, time::Duration};

use futures::{channel::oneshot, future};
use hyper::{Body, Response, Server};
use lazy_static::lazy_static;
use tokio::{task::JoinHandle, time};
use warp::Filter;
use warp_json_rpc::Builder;

use casper_client::{DeployStrParams, PaymentStrParams, SessionStrParams};
use casper_node::rpcs::{
    account::{PutDeploy, PutDeployParams},
    info::{GetDeploy, GetDeployParams},
    RpcWithParams,
};

fn handle_put_deploy(response_builder: Builder, _params: PutDeployParams) -> Response<Body> {
    response_builder.success(()).unwrap()
}

fn handle_get_deploy(response_builder: Builder, _params: GetDeployParams) -> Response<Body> {
    response_builder.success(()).unwrap()
}

lazy_static! {
    /// This is used to serialize tests, as we take the lock at the start of a test and release when complete.
    /// Alternatively, setup your test to use a unique port.
    static ref TEST_LOCK_PORT: Mutex<u16> = Mutex::new(9090);
}

fn spawn_warp_mock_server(
    port: u16,
) -> (
    oneshot::Sender<()>,
    JoinHandle<Result<(), hyper::error::Error>>,
) {
    let rpc_put_deploy = warp_json_rpc::filters::json_rpc()
        .and(warp_json_rpc::filters::method(PutDeploy::METHOD))
        .and(warp_json_rpc::filters::params::<PutDeployParams>())
        .map(handle_put_deploy);

    let rpc_get_deploy = warp_json_rpc::filters::json_rpc()
        .and(warp_json_rpc::filters::method(GetDeploy::METHOD))
        .and(warp_json_rpc::filters::params::<GetDeployParams>())
        .map(handle_get_deploy);

    let service = warp_json_rpc::service(rpc_put_deploy.or(rpc_get_deploy));
    let builder = Server::try_bind(&([127, 0, 0, 1], port).into()).unwrap();
    let make_svc =
        hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));
    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();
    let server = builder.serve(make_svc);
    let server_with_shutdown = server.with_graceful_shutdown(async {
        shutdown_receiver.await.ok();
    });
    let server_joiner = tokio::spawn(server_with_shutdown);
    (shutdown_sender, server_joiner)
}

#[tokio::test(threaded_scheduler)] // needed to spawn the mock server in the background
async fn client_should_send_get_deploy() {
    let lock = TEST_LOCK_PORT.lock().unwrap();
    let node_port = *lock;
    let (graceful_shutdown, join_handle) = spawn_warp_mock_server(node_port);
    let response = time::timeout(Duration::from_secs(1), async move {
        casper_client::get_deploy(
            "1",
            &format!("http://localhost:{}", node_port),
            false,
            "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
        )
    })
    .await
    .unwrap();
    assert_eq!(
        response
            .expect("parsed request values")
            .get_result()
            .map(|val| serde_json::from_value::<()>(val.clone()).unwrap()),
        Some(())
    );

    let _ = graceful_shutdown.send(());
    let _ = join_handle.await;
}

#[tokio::test(threaded_scheduler)]
async fn client_should_send_put_deploy() {
    let lock = TEST_LOCK_PORT.lock().unwrap();
    let node_port = *lock;
    let (graceful_shutdown, join_handle) = spawn_warp_mock_server(node_port);
    let response = time::timeout(Duration::from_secs(1), async move {
        casper_client::put_deploy(
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
        )
    })
    .await
    .unwrap();
    assert_eq!(
        response
            .expect("parsed request values")
            .get_result()
            .map(|val| serde_json::from_value::<()>(val.clone()).unwrap()),
        Some(())
    );

    let _ = graceful_shutdown.send(());
    let _ = join_handle.await;
}
