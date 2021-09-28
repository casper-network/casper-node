use std::{convert::Infallible, fs, net::SocketAddr, sync::Arc, time::Duration};

use futures::{channel::oneshot, future};
use hyper::{Body, Response, Server};
use serde::Deserialize;
use tempfile::TempDir;
use tokio::{sync::Mutex, task::JoinHandle};
use tower::builder::ServiceBuilder;
use warp::{Filter, Rejection};
use warp_json_rpc::Builder;

use casper_node::crypto::Error as CryptoError;

use casper_client::{
    DeployStrParams, DictionaryItemStrParams, Error, GlobalStateStrParams, PaymentStrParams,
    SessionStrParams,
};
use casper_node::rpcs::{
    account::{PutDeploy, PutDeployParams},
    chain::{GetStateRootHash, GetStateRootHashParams},
    info::{GetDeploy, GetDeployParams},
    state::{GetBalance, GetBalanceParams, GetDictionaryItem, GetDictionaryItemParams},
    RpcWithOptionalParams, RpcWithParams,
};

const VALID_PURSE_UREF: &str =
    "uref-0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20-007";
const VALID_STATE_ROOT_HASH: &str =
    "55db08058acb54c295b115cbd9b282eb2862e76d5bb8493bb80c0598a50a12a5";

const DEFAULT_RATE_LIMIT: u64 = 1;
const DEFAULT_RATE_PER: Duration = Duration::from_secs(1);

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

type ServerJoiner = Option<Arc<Mutex<JoinHandle<Result<(), hyper::Error>>>>>;

struct MockServerHandle {
    graceful_shutdown: Option<oneshot::Sender<()>>,
    server_joiner: ServerJoiner,
    address: SocketAddr,
}

trait Captures<'a> {}
impl<'a, T: ?Sized> Captures<'a> for T {}

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
        Self::spawn_with_filter(
            test_filter::<P>(method),
            DEFAULT_RATE_LIMIT,
            DEFAULT_RATE_PER,
        )
    }

    /// Will spawn a server on localhost and respond to JSON-RPC requests that don't take
    /// parameters.
    fn spawn_without_params(method: &'static str) -> Self {
        Self::spawn_with_filter(
            test_filter_without_params(method),
            DEFAULT_RATE_LIMIT,
            DEFAULT_RATE_PER,
        )
    }

    fn spawn_with_filter<F>(filter: F, rate: u64, per: Duration) -> Self
    where
        F: Filter<Extract = (Response<Body>,), Error = Rejection> + Send + Sync + 'static + Copy,
    {
        let service = warp_json_rpc::service(filter);

        let make_svc =
            hyper::service::make_service_fn(move |_| future::ok::<_, Infallible>(service.clone()));

        let make_svc = ServiceBuilder::new()
            .rate_limit(rate, per)
            .service(make_svc);

        let builder = Server::try_bind(&([127, 0, 0, 1], 0).into()).unwrap();
        let (graceful_shutdown, shutdown_receiver) = oneshot::channel::<()>();
        let graceful_shutdown = Some(graceful_shutdown);
        let server = builder.serve(make_svc);
        let address = server.local_addr();
        let server_with_shutdown = server.with_graceful_shutdown(async {
            shutdown_receiver.await.ok();
        });
        let server_joiner = tokio::spawn(server_with_shutdown);
        let server_joiner = Some(Arc::new(Mutex::new(server_joiner)));
        MockServerHandle {
            graceful_shutdown,
            server_joiner,
            address,
        }
    }

    async fn get_balance(&self, state_root_hash: &str, purse_uref: &str) -> Result<(), Error> {
        casper_client::get_balance("1", &self.url(), 0, state_root_hash, purse_uref)
            .await
            .map(|_| ())
    }

    async fn get_deploy(&self, deploy_hash: &str) -> Result<(), Error> {
        casper_client::get_deploy("1", &self.url(), 0, deploy_hash)
            .await
            .map(|_| ())
    }

    async fn get_state_root_hash(&self, maybe_block_id: &str) -> Result<(), Error> {
        casper_client::get_state_root_hash("1", &self.url(), 0, maybe_block_id)
            .await
            .map(|_| ())
    }

    async fn get_block(&self, maybe_block_id: &str) -> Result<(), Error> {
        casper_client::get_block("1", &self.url(), 0, maybe_block_id)
            .await
            .map(|_| ())
    }

    #[allow(deprecated)]
    async fn get_item(&self, state_root_hash: &str, key: &str, path: &str) -> Result<(), Error> {
        casper_client::get_item("1", &self.url(), 0, state_root_hash, key, path)
            .await
            .map(|_| ())
    }

    async fn query_global_state(
        &self,
        global_state_params: GlobalStateStrParams<'_>,
        key: &str,
        path: &str,
    ) -> Result<(), Error> {
        casper_client::query_global_state("1", &self.url(), 0, global_state_params, key, path)
            .await
            .map(|_| ())
    }

    async fn get_dictionary_item(
        &self,
        state_root_hash: &str,
        dictionary_str_params: DictionaryItemStrParams<'_>,
    ) -> Result<(), Error> {
        casper_client::get_dictionary_item(
            "1",
            &self.url(),
            0,
            state_root_hash,
            dictionary_str_params,
        )
        .await
        .map(|_| ())
    }

    async fn transfer(
        &self,
        amount: &str,
        target_account: &str,
        deploy_params: DeployStrParams<'_>,
        payment_params: PaymentStrParams<'_>,
    ) -> Result<(), Error> {
        casper_client::transfer(
            "1",
            &self.url(),
            0,
            amount,
            target_account,
            "2",
            deploy_params,
            payment_params,
        )
        .await
        .map(|_| ())
    }

    async fn put_deploy(
        &self,
        deploy_params: DeployStrParams<'_>,
        session_params: SessionStrParams<'_>,
        payment_params: PaymentStrParams<'_>,
    ) -> Result<(), Error> {
        casper_client::put_deploy(
            "1",
            &self.url(),
            0,
            deploy_params,
            session_params,
            payment_params,
        )
        .await
        .map(|_| ())
    }

    async fn send_deploy_file(&self, input_path: &str) -> Result<(), Error> {
        casper_client::send_deploy_file("1", &self.url(), 0, input_path)
            .await
            .map(|_| ())
    }

    async fn get_auction_info(&self, maybe_block_id: &str) -> Result<(), Error> {
        casper_client::get_auction_info("1", &self.url(), 0, maybe_block_id)
            .await
            .map(|_| ())
    }

    async fn get_validator_changes(&self) -> Result<(), Error> {
        casper_client::get_validator_changes("1", &self.url(), 0)
            .await
            .map(|_| ())
    }
}

impl Drop for MockServerHandle {
    fn drop(&mut self) {
        let _ = self.graceful_shutdown.take().unwrap().send(());
        let joiner = self.server_joiner.take().unwrap();
        futures::executor::block_on(async {
            let join = &mut *joiner.lock().await;
            let _ = join.await;
        });
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

/// Sample data creation methods for GlobalStateStrParams
mod global_state_params {
    use super::*;

    pub fn test_params_as_state_root_hash() -> GlobalStateStrParams<'static> {
        GlobalStateStrParams {
            is_block_hash: false,
            hash_value: VALID_STATE_ROOT_HASH,
        }
    }

    pub fn invalid_global_state_str_params() -> GlobalStateStrParams<'static> {
        GlobalStateStrParams {
            is_block_hash: false,
            hash_value: "invalid state root hash",
        }
    }
}

/// Sample data creation methods for DictionaryItemStrParams.
mod dictionary_item_str_params {
    use super::*;

    const DICTIONARY_NAME: &str = "test-dictionary";
    const DICTIONARY_ITEM_KEY: &str = "test-item";

    pub fn generate_valid_account_params() -> DictionaryItemStrParams<'static> {
        DictionaryItemStrParams::AccountNamedKey {
            key: "account-hash-09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
            dictionary_name: DICTIONARY_NAME,
            dictionary_item_key: DICTIONARY_ITEM_KEY,
        }
    }

    pub fn generate_invalid_account_params() -> DictionaryItemStrParams<'static> {
        DictionaryItemStrParams::AccountNamedKey {
            key: "invalid account hash",
            dictionary_name: DICTIONARY_NAME,
            dictionary_item_key: DICTIONARY_ITEM_KEY,
        }
    }

    pub fn generate_valid_contract_params() -> DictionaryItemStrParams<'static> {
        DictionaryItemStrParams::ContractNamedKey {
            key: "hash-09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
            dictionary_name: DICTIONARY_NAME,
            dictionary_item_key: DICTIONARY_ITEM_KEY,
        }
    }

    pub fn generate_invalid_contract_params() -> DictionaryItemStrParams<'static> {
        DictionaryItemStrParams::ContractNamedKey {
            key: "invalid contract hash",
            dictionary_name: DICTIONARY_NAME,
            dictionary_item_key: DICTIONARY_ITEM_KEY,
        }
    }

    pub fn generate_valid_uref_params() -> DictionaryItemStrParams<'static> {
        DictionaryItemStrParams::URef {
            seed_uref: VALID_PURSE_UREF,
            dictionary_item_key: DICTIONARY_ITEM_KEY,
        }
    }

    pub fn generate_invalid_uref_params() -> DictionaryItemStrParams<'static> {
        DictionaryItemStrParams::URef {
            seed_uref: "invalid uref",
            dictionary_item_key: DICTIONARY_ITEM_KEY,
        }
    }

    pub fn generate_valid_dictionary_address() -> DictionaryItemStrParams<'static> {
        DictionaryItemStrParams::Dictionary(
            "dictionary-09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
        )
    }

    pub fn generate_invalid_dictionary_address() -> DictionaryItemStrParams<'static> {
        DictionaryItemStrParams::Dictionary("invalid dictionary address")
    }
}

mod get_balance {
    use super::*;

    use casper_client::ValidateResponseError;
    use casper_types::URefFromStrError;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_arguments() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert!(matches!(
            server_handle
                .get_balance(VALID_STATE_ROOT_HASH, VALID_PURSE_UREF)
                .await,
            // NOTE: this "success" means that we then fail to validate the response, but that
            // is outside the scope of this test.
            // The MockServerHandle could support a pre-baked response, which should successfully
            // validate
            Err(Error::InvalidResponse(
                ValidateResponseError::ValidateResponseFailedToParse
            ))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_empty_arguments() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert!(matches!(
            server_handle.get_balance("", "").await,
            Err(Error::InvalidArgument {
                context: "state_root_hash",
                error: _
            })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_empty_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert!(matches!(
            server_handle.get_balance("", VALID_PURSE_UREF).await,
            Err(Error::InvalidArgument {
                context: "state_root_hash",
                error: _
            })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_empty_purse_uref() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert!(matches!(
            server_handle.get_balance(VALID_STATE_ROOT_HASH, "").await,
            Err(Error::FailedToParseURef {
                context: "purse_uref",
                error: URefFromStrError::InvalidPrefix
            })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_bad_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetBalanceParams>(GetBalance::METHOD);
        assert!(matches!(
            server_handle
                .get_balance("deadbeef", VALID_PURSE_UREF)
                .await,
            Err(Error::InvalidArgument {
                context: "state_root_hash",
                error: _
            })
        ));
    }
}

mod get_state_root_hash {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_block_id() {
        let server_handle =
            MockServerHandle::spawn::<GetStateRootHashParams>(GetStateRootHash::METHOD);
        assert!(matches!(
            server_handle
                .get_state_root_hash(
                    "7a073a340bb5e0ca60f4c1dbb3254fb0641da79cda7c5aeb5303efa74fcc9eb1",
                )
                .await,
            Ok(())
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_empty_block_id() {
        let server_handle = MockServerHandle::spawn_without_params(GetStateRootHash::METHOD);
        assert!(matches!(
            server_handle.get_state_root_hash("").await,
            Ok(())
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_block_height() {
        let server_handle = MockServerHandle::spawn_without_params(GetStateRootHash::METHOD);
        assert!(matches!(
            server_handle.get_state_root_hash("1").await,
            Ok(())
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_bad_block_id() {
        let server_handle = MockServerHandle::spawn_without_params(GetStateRootHash::METHOD);
        let input = "<not a real block id>";
        assert!(
            server_handle.get_state_root_hash(input).await.is_err(),
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

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_block_hash() {
        let server_handle = MockServerHandle::spawn::<GetBlockParams>(GetBlock::METHOD);
        assert!(matches!(
            server_handle.get_block(VALID_STATE_ROOT_HASH).await,
            Err(Error::InvalidResponse(
                ValidateResponseError::NoBlockInResponse
            ))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_block_height() {
        let server_handle = MockServerHandle::spawn::<GetBlockParams>(GetBlock::METHOD);
        assert!(matches!(
            server_handle.get_block("1").await,
            Err(Error::InvalidResponse(
                ValidateResponseError::NoBlockInResponse
            ))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_empty_block_hash() {
        let server_handle = MockServerHandle::spawn_without_params(GetBlock::METHOD);
        assert!(matches!(
            server_handle.get_block("").await,
            Err(Error::InvalidResponse(
                ValidateResponseError::NoBlockInResponse
            ))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_invalid_block_id() {
        let server_handle = MockServerHandle::spawn::<GetBlockParams>(GetBlock::METHOD);
        assert!(matches!(
            server_handle.get_block("<not a valid hash>").await,
            Err(Error::FailedToParseInt {
                context: "block_identifier",
                error: _
            })
        ))
    }
}

mod get_item {
    use casper_client::ValidateResponseError;
    use casper_node::rpcs::state::{GetItem, GetItemParams};

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetItemParams>(GetItem::METHOD);

        // in this case, the error means that the request was sent successfully, but due to to the
        // mock implementation fails to validate

        assert!(matches!(
            server_handle
                .get_item(VALID_STATE_ROOT_HASH, VALID_PURSE_UREF, "")
                .await,
            Err(Error::InvalidResponse(
                ValidateResponseError::ValidateResponseFailedToParse
            ))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_invalid_state_root_hash() {
        let server_handle = MockServerHandle::spawn::<GetItemParams>(GetItem::METHOD);
        assert!(matches!(
            server_handle
                .get_item("<invalid state root hash>", VALID_PURSE_UREF, "")
                .await,
            Err(Error::CryptoError {
                context: "state_root_hash",
                error: CryptoError::FromHex(base16::DecodeError::InvalidLength { length: 25 })
            })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_invalid_key() {
        let server_handle = MockServerHandle::spawn::<GetItemParams>(GetItem::METHOD);
        assert!(matches!(
            server_handle
                .get_item(VALID_STATE_ROOT_HASH, "invalid key", "")
                .await,
            Err(Error::FailedToParseKey)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_empty_key() {
        let server_handle = MockServerHandle::spawn::<GetItemParams>(GetItem::METHOD);
        assert!(matches!(
            server_handle
                .get_item("<invalid state root hash>", "", "")
                .await,
            Err(Error::CryptoError {
                context: "state_root_hash",
                error: CryptoError::FromHex(base16::DecodeError::InvalidLength { length: 25 })
            })
        ));
    }
}

mod get_dictionary_item {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_dictionary_params() {
        let server_handle =
            MockServerHandle::spawn::<GetDictionaryItemParams>(GetDictionaryItem::METHOD);
        let dictionary_str_account_params =
            dictionary_item_str_params::generate_valid_account_params();
        assert!(matches!(
            server_handle
                .get_dictionary_item(VALID_STATE_ROOT_HASH, dictionary_str_account_params)
                .await,
            Ok(())
        ));

        let dictionary_contract_params =
            dictionary_item_str_params::generate_valid_contract_params();
        assert!(matches!(
            server_handle
                .get_dictionary_item(VALID_STATE_ROOT_HASH, dictionary_contract_params)
                .await,
            Ok(())
        ));

        let dictionary_uref_params = dictionary_item_str_params::generate_valid_uref_params();
        assert!(matches!(
            server_handle
                .get_dictionary_item(VALID_STATE_ROOT_HASH, dictionary_uref_params)
                .await,
            Ok(())
        ));

        let dictionary_address_params =
            dictionary_item_str_params::generate_valid_dictionary_address();
        assert!(matches!(
            server_handle
                .get_dictionary_item(VALID_STATE_ROOT_HASH, dictionary_address_params)
                .await,
            Ok(())
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_invalid_params() {
        let server_handle =
            MockServerHandle::spawn::<GetDictionaryItemParams>(GetDictionaryItem::METHOD);
        let invalid_dictionary_account_params =
            dictionary_item_str_params::generate_invalid_account_params();

        assert!(matches!(
            server_handle
                .get_dictionary_item(VALID_STATE_ROOT_HASH, invalid_dictionary_account_params)
                .await,
            Err(Error::FailedToParseDictionaryIdentifier)
        ));

        let invalid_dictionary_contract_params =
            dictionary_item_str_params::generate_invalid_contract_params();
        assert!(matches!(
            server_handle
                .get_dictionary_item(VALID_STATE_ROOT_HASH, invalid_dictionary_contract_params)
                .await,
            Err(Error::FailedToParseDictionaryIdentifier)
        ));

        let invalid_dictionary_uref_params =
            dictionary_item_str_params::generate_invalid_uref_params();
        assert!(matches!(
            server_handle
                .get_dictionary_item(VALID_STATE_ROOT_HASH, invalid_dictionary_uref_params)
                .await,
            Err(Error::FailedToParseDictionaryIdentifier)
        ));

        let invalid_dictionary_address =
            dictionary_item_str_params::generate_invalid_dictionary_address();
        assert!(matches!(
            server_handle
                .get_dictionary_item(VALID_STATE_ROOT_HASH, invalid_dictionary_address)
                .await,
            Err(Error::FailedToParseDictionaryIdentifier)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_invalid_state_root_hash() {
        let server_handle =
            MockServerHandle::spawn::<GetDictionaryItemParams>(GetDictionaryItem::METHOD);
        let dictionary_params = dictionary_item_str_params::generate_valid_account_params();
        assert!(matches!(
            server_handle
                .get_dictionary_item("<invalid state root hash>", dictionary_params)
                .await,
            Err(Error::CryptoError {
                context: "state_root_hash",
                error: CryptoError::FromHex(base16::DecodeError::InvalidLength { length: _ })
            })
        ));
    }
}

mod query_global_state {
    use casper_client::ValidateResponseError;
    use casper_node::rpcs::state::{QueryGlobalState, QueryGlobalStateParams};

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_global_state_params() {
        let server_handle =
            MockServerHandle::spawn::<QueryGlobalStateParams>(QueryGlobalState::METHOD);

        // in this case, the error means that the request was sent successfully, but due to to the
        // mock implementation fails to validate

        assert!(matches!(
            server_handle
                .query_global_state(
                    global_state_params::test_params_as_state_root_hash(),
                    VALID_PURSE_UREF,
                    ""
                )
                .await,
            Err(Error::InvalidResponse(
                ValidateResponseError::ValidateResponseFailedToParse
            ))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_invalid_global_state_params() {
        let server_handle =
            MockServerHandle::spawn::<QueryGlobalStateParams>(QueryGlobalState::METHOD);
        assert!(matches!(
            server_handle
                .query_global_state(
                    global_state_params::invalid_global_state_str_params(),
                    VALID_PURSE_UREF,
                    ""
                )
                .await,
            Err(Error::CryptoError {
                context: "global_state_identifier",
                error: CryptoError::FromHex(base16::DecodeError::InvalidLength { length: _ })
            })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_invalid_key() {
        let server_handle =
            MockServerHandle::spawn::<QueryGlobalStateParams>(QueryGlobalState::METHOD);
        assert!(matches!(
            server_handle
                .query_global_state(
                    global_state_params::test_params_as_state_root_hash(),
                    "invalid key",
                    ""
                )
                .await,
            Err(Error::FailedToParseKey)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_empty_key() {
        let server_handle =
            MockServerHandle::spawn::<QueryGlobalStateParams>(QueryGlobalState::METHOD);
        assert!(matches!(
            server_handle
                .query_global_state(
                    global_state_params::invalid_global_state_str_params(),
                    "",
                    ""
                )
                .await,
            Err(Error::CryptoError {
                context: "global_state_identifier",
                error: CryptoError::FromHex(base16::DecodeError::InvalidLength { length: _ }),
            })
        ));
    }
}

mod get_deploy {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_with_valid_hash() {
        let server_handle = MockServerHandle::spawn::<GetDeployParams>(GetDeploy::METHOD);
        assert!(matches!(
            server_handle
                .get_deploy("09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6")
                .await,
            Ok(())
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_invalid_hash() {
        let server_handle = MockServerHandle::spawn::<GetDeployParams>(GetDeploy::METHOD);
        assert!(matches!(
            server_handle.get_deploy("012345",).await,
            Err(Error::InvalidArgument {
                context: "deploy",
                // error: "The deploy hash provided had an invalid length of 6."
                error: _
            })
        ));
    }
}

mod get_auction_info {
    use super::*;

    use casper_node::rpcs::state::GetAuctionInfo;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed() {
        let server_handle = MockServerHandle::spawn_without_params(GetAuctionInfo::METHOD);
        assert!(matches!(server_handle.get_auction_info("").await, Ok(())));
    }
}

mod get_validator_changes {
    use super::*;

    use casper_node::rpcs::{info::GetValidatorChanges, RpcWithoutParams};

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed() {
        let server_handle = MockServerHandle::spawn_without_params(GetValidatorChanges::METHOD);
        assert!(matches!(
            server_handle.get_validator_changes().await,
            Ok(())
        ))
    }
}

mod make_deploy {
    use super::*;

    #[test]
    fn should_succeed_for_stdout() {
        assert!(matches!(
            casper_client::make_deploy(
                "",
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
    }

    #[test]
    fn should_succeed_for_file() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let file_path = temp_dir.path().join("test_deploy.json");

        assert!(matches!(
            casper_client::make_deploy(
                file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
    }

    #[test]
    fn should_fail_with_existing_file() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let file_path = temp_dir.path().join("test_deploy.json");
        let contents = "contents of test file";
        fs::write(file_path.clone(), &contents)
            .unwrap_or_else(|err| panic!("Failed to create temp file with error: {}", err));

        assert!(matches!(
            casper_client::make_deploy(
                file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Err(Error::FileAlreadyExists(_))
        ));

        let contents_after_fail = fs::read_to_string(file_path).unwrap_or_else(|err| {
            panic!("Failed to read contents of test file with error: {}", err)
        });

        assert_eq!(contents, contents_after_fail);
    }

    #[test]
    fn should_succeed_with_existing_file_with_force() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let file_path = temp_dir.path().join("test_deploy.json");
        fs::write(file_path.clone(), "hi")
            .unwrap_or_else(|err| panic!("Failed to create temp file with error: {}", err));

        assert!(matches!(
            casper_client::make_deploy(
                file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                true
            ),
            Ok(())
        ));
    }
}

mod send_deploy {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_fail_with_bad_deploy_file_path() {
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);
        if let Err(Error::IoError { context, .. }) =
            server_handle.send_deploy_file("<not a valid path>").await
        {
            assert_eq!(context, "unable to read input file \'<not a valid path>\'")
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed_for_file() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let file_path = temp_dir.path().join("test_send_deploy.json");
        assert!(matches!(
            casper_client::make_deploy(
                file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);
        assert!(matches!(
            server_handle
                .send_deploy_file(file_path.to_str().unwrap())
                .await,
            Ok(())
        ));
    }
}

mod sign_deploy {
    use super::*;

    #[test]
    fn should_succeed_for_file() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let unsigned_file_path = temp_dir.path().join("test_deploy.json");
        let signed_file_path = temp_dir.path().join("signed_test_deploy.json");
        assert!(matches!(
            casper_client::make_deploy(
                unsigned_file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
        assert!(matches!(
            casper_client::sign_deploy_file(
                unsigned_file_path.to_str().unwrap(),
                "../resources/local/secret_keys/node-1.pem",
                signed_file_path.to_str().unwrap(),
                false
            ),
            Ok(())
        ));
    }

    #[test]
    fn should_succeed_for_stdout() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let unsigned_file_path = temp_dir.path().join("test_deploy.json");
        assert!(matches!(
            casper_client::make_deploy(
                unsigned_file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
        assert!(matches!(
            casper_client::sign_deploy_file(
                unsigned_file_path.to_str().unwrap(),
                "../resources/local/secret_keys/node-1.pem",
                "",
                false
            ),
            Ok(())
        ));
    }

    #[test]
    fn should_fail_with_bad_secret_key_path() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let unsigned_file_path = temp_dir.path().join("test_deploy.json");
        assert!(matches!(
            casper_client::make_deploy(
                unsigned_file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
        assert!(casper_client::sign_deploy_file(
            unsigned_file_path.to_str().unwrap(),
            "<this is not a path>",
            "",
            false
        )
        .is_err());
    }

    #[test]
    fn should_fail_with_existing_file() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let unsigned_file_path = temp_dir.path().join("test_deploy.json");
        let signed_file_path = temp_dir.path().join("signed_test_deploy.json");
        assert!(matches!(
            casper_client::make_deploy(
                unsigned_file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
        assert!(matches!(
            casper_client::sign_deploy_file(
                unsigned_file_path.to_str().unwrap(),
                "../resources/local/secret_keys/node-1.pem",
                signed_file_path.to_str().unwrap(),
                false
            ),
            Ok(())
        ));

        let contents = fs::read_to_string(signed_file_path.clone())
            .unwrap_or_else(|err| panic!("Failed to read contents of file with error: {}", err));

        assert!(matches!(
            casper_client::sign_deploy_file(
                unsigned_file_path.to_str().unwrap(),
                "../resources/local/secret_keys/node-1.pem",
                signed_file_path.to_str().unwrap(),
                false
            ),
            Err(Error::FileAlreadyExists(_))
        ));

        let contents_after_failure = fs::read_to_string(signed_file_path)
            .unwrap_or_else(|err| panic!("Failed to read contents of file with error: {}", err));

        assert_eq!(contents, contents_after_failure);
    }

    #[test]
    fn should_succeed_with_existing_file_with_force() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let unsigned_file_path = temp_dir.path().join("test_deploy.json");
        let signed_file_path = temp_dir.path().join("signed_test_deploy.json");
        assert!(matches!(
            casper_client::make_deploy(
                unsigned_file_path.to_str().unwrap(),
                deploy_params::test_data_valid(),
                session_params::test_data_with_package_hash(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
        assert!(matches!(
            casper_client::sign_deploy_file(
                unsigned_file_path.to_str().unwrap(),
                "../resources/local/secret_keys/node-1.pem",
                signed_file_path.to_str().unwrap(),
                false
            ),
            Ok(())
        ));
        assert!(matches!(
            casper_client::sign_deploy_file(
                unsigned_file_path.to_str().unwrap(),
                "../resources/local/secret_keys/node-1.pem",
                signed_file_path.to_str().unwrap(),
                true
            ),
            Ok(())
        ));
    }
}

mod make_transfer {
    use super::*;

    const AMOUNT: &str = "30000000000";
    const TARGET_ACCOUNT: &str =
        "01aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TRANSFER_ID: &str = "1314";

    #[test]
    fn should_succeed_for_stdout() {
        assert!(matches!(
            casper_client::make_transfer(
                "",
                AMOUNT,
                TARGET_ACCOUNT,
                TRANSFER_ID,
                deploy_params::test_data_valid(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
    }

    #[test]
    fn should_succeed_for_file() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let file_path = temp_dir.path().join("test_deploy.json");
        assert!(matches!(
            casper_client::make_transfer(
                file_path.to_str().unwrap(),
                AMOUNT,
                TARGET_ACCOUNT,
                TRANSFER_ID,
                deploy_params::test_data_valid(),
                payment_params::test_data_with_name(),
                false
            ),
            Ok(())
        ));
    }

    #[test]
    fn should_fail_with_existing_file() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let file_path = temp_dir.path().join("test_deploy.json");
        let contents = "contents of test file";
        fs::write(file_path.clone(), &contents)
            .unwrap_or_else(|err| panic!("Failed to create temp file with error: {}", err));

        assert!(matches!(
            casper_client::make_transfer(
                file_path.to_str().unwrap(),
                AMOUNT,
                TARGET_ACCOUNT,
                TRANSFER_ID,
                deploy_params::test_data_valid(),
                payment_params::test_data_with_name(),
                false
            ),
            Err(Error::FileAlreadyExists(_))
        ));

        let contents_after_fail = fs::read_to_string(file_path)
            .unwrap_or_else(|err| panic!("Failed to read from temp file with error: {}", err));

        assert_eq!(contents, contents_after_fail);
    }

    #[test]
    fn should_succeed_with_existing_file_with_force() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create temp dir with error: {}", err));
        let file_path = temp_dir.path().join("test_deploy.json");
        fs::write(file_path.clone(), "hi")
            .unwrap_or_else(|err| panic!("Failed to create temp file with error: {}", err));

        assert!(matches!(
            casper_client::make_transfer(
                file_path.to_str().unwrap(),
                AMOUNT,
                TARGET_ACCOUNT,
                TRANSFER_ID,
                deploy_params::test_data_valid(),
                payment_params::test_data_with_name(),
                true
            ),
            Ok(())
        ));
    }
}

mod keygen_generate_files {
    use super::*;

    #[test]
    fn should_succeed_for_valid_args_ed25519() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create a temp dir with error: {}", err));
        let path = temp_dir.path().join("test-keygen-ed25519");
        let result = casper_client::keygen::generate_files(
            path.to_str().unwrap(),
            casper_client::keygen::ED25519,
            true,
        );
        assert!(matches!(result, Ok(())))
    }

    #[test]
    fn should_succeed_for_valid_args_secp256k1() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create a temp dir with error: {}", err));
        let path = temp_dir.path().join("test-keygen-secp256k1");
        let result = casper_client::keygen::generate_files(
            path.to_str().unwrap(),
            casper_client::keygen::SECP256K1,
            true,
        );

        assert!(matches!(result, Ok(())));
    }

    #[test]
    fn should_force_overwrite_when_set() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create a temp dir with error: {}", err));
        let path = temp_dir
            .path()
            .canonicalize()
            .unwrap_or_else(|err| panic!("Failed to canonicalize path of temp dir: {}", err))
            .join("test-keygen-force");
        let result = casper_client::keygen::generate_files(
            path.to_str().unwrap(),
            casper_client::keygen::SECP256K1,
            false,
        );
        assert!(matches!(result, Ok(())));
        let result = casper_client::keygen::generate_files(
            path.to_str().unwrap(),
            casper_client::keygen::SECP256K1,
            false,
        );
        assert!(matches!(result, Err(Error::FileAlreadyExists(_))));
        let result = casper_client::keygen::generate_files(
            path.to_str().unwrap(),
            casper_client::keygen::SECP256K1,
            true,
        );
        assert!(matches!(result, Ok(())));
    }

    #[test]
    fn should_fail_for_invalid_algorithm() {
        let temp_dir = TempDir::new()
            .unwrap_or_else(|err| panic!("Failed to create a temp dir with error: {}", err));
        let path = temp_dir.path().join("test-keygen-invalid-algo");
        let result = casper_client::keygen::generate_files(
            path.to_str().unwrap(),
            "<not a valid algo>",
            true,
        );
        assert!(matches!(result, Err(Error::UnsupportedAlgorithm(_))));
    }

    #[test]
    fn should_fail_for_invalid_output_dir() {
        let path = "";
        let result =
            casper_client::keygen::generate_files(path, casper_client::keygen::ED25519, true);
        assert!(matches!(result, Err(Error::InvalidArgument { .. })))
    }
}

mod put_deploy {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_send_put_deploy() {
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);
        assert!(matches!(
            server_handle
                .put_deploy(
                    deploy_params::test_data_valid(),
                    session_params::test_data_with_package_hash(),
                    payment_params::test_data_with_name()
                )
                .await,
            Ok(())
        ));
    }
}

mod rate_limit {
    use super::*;
    use casper_node::types::Timestamp;

    #[tokio::test(flavor = "multi_thread")]
    async fn client_should_should_be_rate_limited_to_approx_1_qps() {
        // Transfer uses PutDeployParams + PutDeploy
        let server_handle = Arc::new(MockServerHandle::spawn::<PutDeployParams>(
            PutDeploy::METHOD,
        ));

        let now = Timestamp::now();
        // Our default is 1 req/s, so this will hit the threshold
        for _ in 0..3u32 {
            let amount = "100";
            let target_account =
                "01522ef6c89038019cb7af05c340623804392dd2bb1f4dab5e4a9c3ab752fc0179";

            let server_handle = server_handle.clone();

            assert!(server_handle
                .transfer(
                    amount,
                    target_account,
                    deploy_params::test_data_valid(),
                    payment_params::test_data_with_name(),
                )
                .await
                .is_ok());
        }

        let diff = now.elapsed();
        assert!(
            diff < Duration::from_millis(4000).into(),
            "Rate limiting of 1 qps for 3 sec took too long at {}ms",
            diff.millis()
        );
        assert!(
            diff > Duration::from_millis(2000).into(),
            "Rate limiting of 1 qps for 3 sec too fast took {}ms",
            diff.millis()
        );
    }
}

mod transfer {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn should_succeed() {
        // // Transfer uses PutDeployParams + PutDeploy
        let server_handle = MockServerHandle::spawn::<PutDeployParams>(PutDeploy::METHOD);
        let amount = "100";
        let target_account = "01522ef6c89038019cb7af05c340623804392dd2bb1f4dab5e4a9c3ab752fc0179";
        assert!(server_handle
            .transfer(
                amount,
                target_account,
                deploy_params::test_data_valid(),
                payment_params::test_data_with_name()
            )
            .await
            .is_ok());
    }
}
