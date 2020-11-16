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
    state::{GetBalance, GetBalanceParams},
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
impl MockServerHandle {
    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
    /// Will spawn a server on localhost and respond to JSON-RPC requests that successfully
    /// deserialize as `P`.
    fn spawn<P>(method: &'static str) -> Self
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

    fn get_balance(&self, purse_uref: &str, state_root_hash: &str) -> Result<(), ErrWrapper> {
        casper_client::get_balance("1", &self.url(), false, purse_uref, state_root_hash)
            .map(|_| ())
            .map_err(ErrWrapper)
    }

    fn get_deploy(&self, deploy_hash: &str) -> Result<(), ErrWrapper> {
        casper_client::get_deploy("1", &self.url(), false, deploy_hash)
            .map(|_| ())
            .map_err(ErrWrapper)
    }
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

/// NewType wrapper for client errors allowing impl of PartialEq for assert_eq! and test variants.
#[derive(Debug)]
struct ErrWrapper(Error);

impl PartialEq for ErrWrapper {
    fn eq(&self, other: &ErrWrapper) -> bool {
        format!("{:?}", self.0) == format!("{:?}", other.0)
    }
}

impl Into<ErrWrapper> for Error {
    fn into(self) -> ErrWrapper {
        ErrWrapper(self)
    }
}

const VALID_PURSE_UREF: &str =
    "uref-0127d7f966ee38a85aaf7cd869c51553f7e1a162ede9d772899fbef2f59da2f085-123";

const VALID_STATE_HASH: &str = "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF";

use casper_node::crypto::Error::*;
use hex::FromHexError::*;
use Error::*;

mod get_balance {
    use super::*;

    #[tokio::test(threaded_scheduler)]
    async fn should_succeed_with_valid_arguments() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert!(server_handle
            .get_balance(VALID_PURSE_UREF, VALID_STATE_HASH)
            .is_ok());
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_empty_arguments() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance("", ""),
            Err(CryptoError("purse_uref", FromHex(InvalidStringLength)).into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_empty_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance(VALID_PURSE_UREF, ""),
            Err(CryptoError("state_root_hash", FromHex(OddLength)).into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_empty_purse_uref() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance("", VALID_STATE_HASH),
            Err(CryptoError("purse_uref", FromHex(InvalidStringLength)).into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_bad_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance(VALID_PURSE_UREF, "deadbeef"),
            Err(CryptoError("state_root_hash", FromHex(InvalidStringLength)).into())
        );
    }
}

mod get_state_root_hash {
    use super::*;

    #[tokio::test(threaded_scheduler)]
    async fn client_should_send_get_state_root_hash() {
        let server_handle =
            MockServerHandle::spawn::<GetStateRootHashParams>(GetStateRootHash::METHOD);

        let response = casper_client::get_state_root_hash(
            "1",
            &server_handle.url(),
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
}
mod get_deploy {
    use super::*;

    #[tokio::test(threaded_scheduler)]
    async fn get_deploy_should_succeed_with_valid_hash() {
        let server_handle = MockServerHandle::spawn::<GetDeployParams>(GetDeploy::METHOD);
        assert_eq!(
            server_handle
                .get_deploy("09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",),
            Ok(())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn get_deploy_should_fail_with_invalid_hash() {
        let server_handle = MockServerHandle::spawn::<GetDeployParams>(GetDeploy::METHOD);
        assert_eq!(
            server_handle.get_deploy("012345",),
            Err(CryptoError("deploy_hash", FromHex(InvalidStringLength)).into())
        );
    }
}

mod put_deploy {
    use super::*;

    #[tokio::test(threaded_scheduler)]
    async fn client_should_send_put_deploy() {
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);

        let response = casper_client::put_deploy(
            "1",
            &server_handle.url(),
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
}

mod transfer {
    use super::*;

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
}
