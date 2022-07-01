use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use futures::FutureExt;
use serde::Serialize;
use serde_json::Value;
use tracing::{debug, error};

use crate::{
    error::{Error, ReservedErrorCode},
    request::{Params, Request},
    response::Response,
};

/// A boxed future of `Result<Value, Error>`; the return type of a request-handling closure.
type HandleRequestFuture = Pin<Box<dyn Future<Output = Result<Value, Error>> + Send>>;
/// A request-handling closure.
type RequestHandler = Arc<dyn Fn(Option<Params>) -> HandleRequestFuture + Send + Sync>;

/// A collection of request-handlers, indexed by the JSON-RPC "method" applicable to each.
///
/// There needs to be a unique handler for each JSON-RPC request "method" to be handled.  Handlers
/// are added via a [`RequestHandlersBuilder`].
#[derive(Clone)]
pub struct RequestHandlers(Arc<HashMap<&'static str, RequestHandler>>);

impl RequestHandlers {
    /// Finds the relevant handler for the given request's "method" field, and invokes it with the
    /// given "params" value.
    ///
    /// If a handler cannot be found, a MethodNotFound error is created.  In this case, or if
    /// invoking the handler yields an [`Error`], the error is converted into a
    /// [`Response::Failure`].
    ///
    /// Otherwise a [`Response::Success`] is returned.
    pub(crate) async fn handle_request(&self, request: Request) -> Response {
        let handler = match self.0.get(request.method.as_str()) {
            Some(handler) => Arc::clone(handler),
            None => {
                debug!(requested_method = %request.method.as_str(), "failed to get handler");
                let error = Error::new(
                    ReservedErrorCode::MethodNotFound,
                    format!(
                        "'{}' is not a supported json-rpc method on this server",
                        request.method.as_str()
                    ),
                );
                return Response::new_failure(request.id, error);
            }
        };

        match handler(request.params).await {
            Ok(result) => Response::new_success(request.id, result),
            Err(error) => Response::new_failure(request.id, error),
        }
    }
}

/// A builder for [`RequestHandlers`].
//
// This builder exists so the internal `HashMap` can be populated before it is made immutable behind
// the `Arc` in the `RequestHandlers`.
#[derive(Default)]
pub struct RequestHandlersBuilder(HashMap<&'static str, RequestHandler>);

impl RequestHandlersBuilder {
    /// Returns a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a new request-handler which will be called to handle all JSON-RPC requests with the
    /// given "method" field.
    ///
    /// The handler should be an async closure or function with a signature like:
    /// ```ignore
    /// async fn handle_it(params: Option<Params>) -> Result<T, Error>
    /// ```
    /// where `T` implements `Serialize` and will be used as the JSON-RPC response's "result" field.
    pub fn register_handler<Func, Fut, T>(&mut self, method: &'static str, handler: Arc<Func>)
    where
        Func: Fn(Option<Params>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, Error>> + Send,
        T: Serialize + 'static,
    {
        let handler = Arc::clone(&handler);
        // The provided handler returns a future with output of `Result<T, Error>`. We need to
        // convert that to a boxed future with output `Result<Value, Error>` to store it in a
        // homogenous collection.
        let wrapped_handler = move |maybe_params| {
            let handler = Arc::clone(&handler);
            async move {
                let success = Arc::clone(&handler)(maybe_params).await?;
                serde_json::to_value(success).map_err(|error| {
                    error!(%error, "failed to encode json-rpc response value");
                    Error::new(
                        ReservedErrorCode::InternalError,
                        format!("failed to encode json-rpc response value: {}", error),
                    )
                })
            }
            .boxed()
        };
        if self.0.insert(method, Arc::new(wrapped_handler)).is_some() {
            error!(
                method,
                "already registered a handler for this json-rpc request method"
            );
        }
    }

    /// Finalize building by converting `self` to a [`RequestHandlers`].
    pub fn build(self) -> RequestHandlers {
        RequestHandlers(Arc::new(self.0))
    }
}
