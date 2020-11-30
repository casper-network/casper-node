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

const VALID_PURSE_UREF: &str =
    "uref-0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20-007";
const VALID_STATE_ROOT_HASH: &str =
    "55db08058acb54c295b115cbd9b282eb2862e76d5bb8493bb80c0598a50a12a5";

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

fn test_filter_without_params(
    method: &'static str,
) -> impl Filter<Extract = (Response<Body>,), Error = Rejection> + Copy {
    warp_json_rpc::filters::json_rpc()
        .and(warp_json_rpc::filters::method(method))
        .map(|builder: Builder| builder.success(()).unwrap())
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
        Self::spawn_with_filter(test_filter::<P>(method))
    }

    /// Will spawn a server on localhost and respond to JSON-RPC requests that don't take
    /// parameters.
    fn spawn_without_params(method: &'static str) -> Self {
        Self::spawn_with_filter(test_filter_without_params(method))
    }

    fn spawn_with_filter<F>(filter: F) -> Self
    where
        F: Filter<Extract = (Response<Body>,), Error = Rejection> + Send + Sync + 'static + Copy,
    {
        let service = warp_json_rpc::service(filter);
        let make_svc =
            hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));
        let builder = Server::try_bind(&([127, 0, 0, 1], 0).into()).unwrap();
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

    // lib/deploy.rs houses file related operation tests
    // intentionally missing here are:
    // * sign_deploy
    // * send_deploy
    // * make_deploy

    fn get_balance(&self, state_root_hash: &str, purse_uref: &str) -> Result<(), ErrWrapper> {
        casper_client::get_balance("1", &self.url(), false, state_root_hash, purse_uref)
            .map(|_| ())
            .map_err(ErrWrapper)
    }

    fn get_deploy(&self, deploy_hash: &str) -> Result<(), ErrWrapper> {
        casper_client::get_deploy("1", &self.url(), false, deploy_hash)
            .map(|_| ())
            .map_err(ErrWrapper)
    }

    fn get_state_root_hash(&self, maybe_block_id: &str) -> Result<(), ErrWrapper> {
        casper_client::get_state_root_hash("1", &self.url(), false, maybe_block_id)
            .map(|_| ())
            .map_err(ErrWrapper)
    }

    fn get_block(&self, maybe_block_id: &str) -> Result<(), ErrWrapper> {
        casper_client::get_block("1", &self.url(), false, maybe_block_id)
            .map(|_| ())
            .map_err(ErrWrapper)
    }

    fn get_item(&self, state_root_hash: &str, key: &str, path: &str) -> Result<(), ErrWrapper> {
        casper_client::get_item("1", &self.url(), false, state_root_hash, key, path)
            .map(|_| ())
            .map_err(ErrWrapper)
    }

    fn transfer(
        &self,
        amount: &str,
        maybe_source_purse: &str,
        maybe_target_purse: &str,
        maybe_target_account: &str,
        deploy_params: DeployStrParams<'static>,
        payment_params: PaymentStrParams<'static>,
    ) -> Result<(), ErrWrapper> {
        casper_client::transfer(
            "1",
            &self.url(),
            false,
            amount,
            maybe_source_purse,
            maybe_target_purse,
            maybe_target_account,
            "",
            deploy_params,
            payment_params,
        )
        .map(|_| ())
        .map_err(ErrWrapper)
    }

    fn put_deploy(
        &self,
        deploy_params: DeployStrParams<'static>,
        session_params: SessionStrParams<'static>,
        payment_params: PaymentStrParams<'static>,
    ) -> Result<(), ErrWrapper> {
        casper_client::put_deploy(
            "1",
            &self.url(),
            false,
            deploy_params,
            session_params,
            payment_params,
        )
        .map(|_| ())
        .map_err(ErrWrapper)
    }

    fn get_auction_info(&self) -> Result<(), ErrWrapper> {
        casper_client::get_auction_info("1", &self.url(), false)
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

#[derive(Debug)]
struct ErrWrapper(pub Error);

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

mod deploy_params {
    use super::*;

    pub fn test_data_valid() -> DeployStrParams<'static> {
        DeployStrParams {
            secret_key: "../resources/local/secret_keys/node-1.pem",
            ttl: "10s",
            chain_name: "casper-test-chain-name-1",
            gas_price: "1",
            ..Default::default()
        }
    }
}

/// Sample data creation methods for PaymentStrParams
mod payment_params {
    use super::*;

    const NAME: &str = "name";
    const ENTRYPOINT: &str = "entrypoint";

    fn args_simple() -> Vec<&'static str> {
        vec!["name_01:bool='false'", "name_02:i32='42'"]
    }

    pub fn test_data_with_name() -> PaymentStrParams<'static> {
        PaymentStrParams::with_name(NAME, ENTRYPOINT, args_simple(), "")
    }
}

/// Sample data creation methods for SessionStrParams
mod session_params {
    use super::*;

    const PKG_HASH: &str = "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6";
    const ENTRYPOINT: &str = "entrypoint";
    const VERSION: &str = "0.1.0";

    fn args_simple() -> Vec<&'static str> {
        vec!["name_01:bool='false'", "name_02:i32='42'"]
    }

    pub fn test_data_with_package_hash() -> SessionStrParams<'static> {
        SessionStrParams::with_package_hash(PKG_HASH, VERSION, ENTRYPOINT, args_simple(), "")
    }
}

use casper_node::crypto::Error::*;
use hex::FromHexError::*;
use Error::*;

mod get_balance {
    use super::*;

    use casper_client::ValidateResponseError;
    use casper_types::URefFromStrError;

    #[tokio::test(threaded_scheduler)]
    async fn should_succeed_with_valid_arguments() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance(VALID_STATE_ROOT_HASH, VALID_PURSE_UREF),
            // NOTE: this "success" means that we then fail to validate the response, but that
            // is outside the scope of this test.
            // The MockServerHandle could support a pre-baked response, which should successfully
            // validate
            Err(InvalidResponse(ValidateResponseError::ValidateResponseFailedToParse).into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_empty_arguments() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance("", ""),
            Err(CryptoError {
                context: "state_root_hash",
                error: FromHex(InvalidStringLength)
            }
            .into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_empty_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance("", VALID_PURSE_UREF),
            Err(CryptoError {
                context: "state_root_hash",
                error: FromHex(InvalidStringLength)
            }
            .into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_empty_purse_uref() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance(VALID_STATE_ROOT_HASH, ""),
            Err(FailedToParseURef("purse_uref", URefFromStrError::InvalidPrefix).into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_bad_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert_eq!(
            server_handle.get_balance("deadbeef", VALID_PURSE_UREF),
            Err(CryptoError {
                context: "state_root_hash",
                error: FromHex(InvalidStringLength)
            }
            .into())
        );
    }
}

mod get_state_root_hash {
    use super::*;

    #[tokio::test(threaded_scheduler)]
    async fn should_succeed_with_valid_block_id() {
        let server_handle =
            MockServerHandle::spawn::<GetStateRootHashParams>(GetStateRootHash::METHOD);
        assert_eq!(
            server_handle.get_state_root_hash(
                "7a073a340bb5e0ca60f4c1dbb3254fb0641da79cda7c5aeb5303efa74fcc9eb1",
            ),
            Ok(())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_succeed_with_empty_block_id() {
        let server_handle = MockServerHandle::spawn_without_params(GetStateRootHash::METHOD);
        assert_eq!(server_handle.get_state_root_hash(""), Ok(()));
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_succeed_with_block_id_of_height() {
        let server_handle = MockServerHandle::spawn_without_params(GetStateRootHash::METHOD);
        assert_eq!(server_handle.get_state_root_hash("1"), Ok(()));
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_with_bad_block_id() {
        let server_handle = MockServerHandle::spawn_without_params(GetStateRootHash::METHOD);
        let input = "<not a real block id>";
        assert!(
            server_handle.get_state_root_hash(input).is_err(),
            "input '{}' should not parse to a valid block id",
            input
        );
    }
}

mod get_block {
    use casper_client::ValidateResponseError;
    use casper_node::rpcs::chain::{GetBlock, GetBlockParams};

    use super::*;

    // in this case, the error means that the request was sent successfully, but due to to the
    // mock implementation fails to validate

    #[tokio::test(threaded_scheduler)]
    async fn get_block_should_succeed_with_valid_block_id() {
        let server_handle = MockServerHandle::spawn::<GetBlockParams>(GetBlock::METHOD);
        assert_eq!(
            server_handle.get_block(VALID_STATE_ROOT_HASH),
            Err(ErrWrapper(InvalidResponse(
                ValidateResponseError::NoBlockInResponse
            )))
        );
    }
    #[tokio::test(threaded_scheduler)]
    async fn get_block_should_succeed_with_valid_block_height() {
        let server_handle = MockServerHandle::spawn::<GetBlockParams>(GetBlock::METHOD);
        assert_eq!(
            server_handle.get_block("1"),
            Err(ErrWrapper(InvalidResponse(
                ValidateResponseError::NoBlockInResponse
            )))
        );
    }
    #[tokio::test(threaded_scheduler)]
    async fn get_block_should_succeed_with_valid_empty_block_id() {
        let server_handle = MockServerHandle::spawn_without_params(GetBlock::METHOD);
        assert_eq!(
            server_handle.get_block(""),
            Err(ErrWrapper(InvalidResponse(
                ValidateResponseError::NoBlockInResponse
            )))
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn get_block_should_fail_with_invalid_block_id() {
        let server_handle = MockServerHandle::spawn::<GetBlockParams>(GetBlock::METHOD);
        match server_handle.get_block("<not a valid hash>") {
            Err(ErrWrapper(Error::FailedToParseInt("block_identifier", _))) => {}
            other => panic!("incorrect error returned from client {:?}", other),
        }
    }
}

mod get_item {
    use casper_client::ValidateResponseError;
    use casper_node::rpcs::state::{GetItem, GetItemParams};

    use super::*;

    #[tokio::test(threaded_scheduler)]
    async fn get_item_should_succeed_with_valid_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetItemParams>(GetItem::METHOD);

        // in this case, the error means that the request was sent successfully, but due to to the
        // mock implementation fails to validate

        assert_eq!(
            server_handle.get_item(VALID_STATE_ROOT_HASH, VALID_PURSE_UREF, ""),
            Err(
                Error::InvalidResponse(ValidateResponseError::ValidateResponseFailedToParse).into()
            )
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn get_item_should_fail_with_invalid_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetItemParams>(GetItem::METHOD);
        assert_eq!(
            server_handle.get_item("<invalid state root hash>", VALID_PURSE_UREF, ""),
            Err(CryptoError {
                context: "state_root_hash",
                error: FromHex(OddLength)
            }
            .into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn get_item_fail_with_invalid_key() {
        let server_handle = MockServerHandle::spawn::<GetItemParams>(GetItem::METHOD);
        assert_eq!(
            server_handle.get_item(VALID_STATE_ROOT_HASH, "invalid key", ""),
            Err(FailedToParseKey.into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn get_item_should_fail_with_empty_key() {
        let server_handle = MockServerHandle::spawn::<GetItemParams>(GetItem::METHOD);
        assert_eq!(
            server_handle.get_item("<invalid state root hash>", "", ""),
            Err(CryptoError {
                context: "state_root_hash",
                error: FromHex(OddLength)
            }
            .into())
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
                .get_deploy("09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6"),
            Ok(())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn get_deploy_should_fail_with_invalid_hash() {
        let server_handle = MockServerHandle::spawn::<GetDeployParams>(GetDeploy::METHOD);
        assert_eq!(
            server_handle.get_deploy("012345",),
            Err(CryptoError {
                context: "deploy_hash",
                error: FromHex(InvalidStringLength)
            }
            .into())
        );
    }
}

mod get_auction_info {
    use super::*;

    use casper_node::rpcs::{state::GetAuctionInfo, RpcWithoutParams};

    #[tokio::test(threaded_scheduler)]
    async fn should_succeed() {
        let server_handle = MockServerHandle::spawn_without_params(GetAuctionInfo::METHOD);
        assert_eq!(server_handle.get_auction_info(), Ok(()));
    }
}

mod put_deploy {
    use super::*;

    #[tokio::test(threaded_scheduler)]
    async fn client_should_send_put_deploy() {
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);
        assert_eq!(
            server_handle.put_deploy(
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name()
            ),
            Ok(())
        );
    }
}

mod transfer {
    use super::*;

    #[tokio::test(threaded_scheduler)]
    async fn should_succeed() {
        // Transfer uses PutDeployParams + PutDeploy
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);
        let amount = "100";
        let maybe_source_purse = VALID_PURSE_UREF;
        let maybe_target_purse = "";
        let maybe_target_account =
            "01522ef6c89038019cb7af05c340623804392dd2bb1f4dab5e4a9c3ab752fc0179";
        assert_eq!(
            server_handle.transfer(
                amount,
                maybe_source_purse,
                maybe_target_purse,
                maybe_target_account,
                deploy_params::test_data_valid(),
                payment_params::test_data_with_name()
            ),
            Ok(())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_if_both_target_purse_and_target_account_are_provided() {
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);
        let maybe_target_purse = VALID_PURSE_UREF;
        let maybe_target_account = "12345";
        assert_eq!(
            server_handle.transfer(
                "100",
                VALID_PURSE_UREF,
                maybe_target_purse,
                maybe_target_account,
                deploy_params::test_data_valid(),
                payment_params::test_data_with_name()
            ),
            Err(Error::InvalidArgument("target_account | target_purse",
            format!(
                "Invalid arguments to get_transfer_target - must provide either a target account or purse. account={}, purse={}", maybe_target_account, maybe_target_purse)).into())
        );
    }

    #[tokio::test(threaded_scheduler)]
    async fn should_fail_if_both_target_purse_and_target_account_are_excluded() {
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);
        assert_eq!(
            server_handle.transfer(
                "100",
                VALID_PURSE_UREF,
                "",
                "",
                deploy_params::test_data_valid(),
                payment_params::test_data_with_name()
            ),
            Err(Error::InvalidArgument(
                "target_account | target_purse",
                "Invalid arguments to get_transfer_target - must provide either a target account or purse. account=, purse=".to_string()).into())
        );
    }
}
