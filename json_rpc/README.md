# `casper-json-rpc`

[![LOGO](https://raw.githubusercontent.com/casper-network/casper-node/master/images/casper-association-logo-primary.svg)](https://casper.network/)

[![Build Status](https://drone-auto-casper-network.casperlabs.io/api/badges/casper-network/casper-node/status.svg?branch=dev)](http://drone-auto-casper-network.casperlabs.io/casper-network/casper-node)
[![Crates.io](https://img.shields.io/crates/v/casper-json-rpc)](https://crates.io/crates/casper-json-rpc)
[![Documentation](https://docs.rs/casper-node/badge.svg)](https://docs.rs/casper-json-rpc)
[![License](https://img.shields.io/badge/license-Apache-blue)](https://github.com/casper-network/casper-node/blob/master/LICENSE)

A library suitable for use as the framework for a JSON-RPC server.

# Usage

Normally usage will involve two steps:
  * construct a set of request handlers using a
    [`RequestHandlersBuilder`](https://docs.rs/casper-json-rpc/latest/casper_json_rpc/struct.RequestHandlersBuilder.html)
  * call [`casper_json_rpc::route`](https://docs.rs/casper-json-rpc/latest/casper_json_rpc/fn.route.html) to construct a
    boxed warp filter ready to be passed to [`warp::service`](https://docs.rs/warp/latest/warp/fn.service.html) for
    example

# Example

```rust
use casper_json_rpc::{Error, Params, RequestHandlersBuilder};
use std::{convert::Infallible, sync::Arc};

async fn get(params: Option<Params>) -> Result<String, Error> {
    // * parse params or return `ReservedErrorCode::InvalidParams` error
    // * handle request and return result
    Ok("got it".to_string())
}

async fn put(params: Option<Params>, other_input: &str) -> Result<String, Error> {
    Ok(other_input.to_string())
}

#[tokio::main]
async fn main() {
    // Register handlers for methods "get" and "put".
    let mut handlers = RequestHandlersBuilder::new();
    handlers.register_handler("get", Arc::new(get));
    let put_handler = move |params| async move { put(params, "other input").await };
    handlers.register_handler("put", Arc::new(put_handler));
    let handlers = handlers.build();

    // Get the new route.
    let path = "rpc";
    let max_body_bytes = 1024;
    let route = casper_json_rpc::route(path, max_body_bytes, handlers);

    // Convert it into a `Service` and run it.
    let make_svc = hyper::service::make_service_fn(move |_| {
        let svc = warp::service(route.clone());
        async move { Ok::<_, Infallible>(svc.clone()) }
    });

    hyper::Server::bind(&([127, 0, 0, 1], 3030).into())
        .serve(make_svc)
        .await
        .unwrap();
}
```

If this receives a request such as

```
curl -X POST -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":"id","method":"get"}' http://127.0.0.1:3030/rpc
```

then the server will respond with

```json
{"jsonrpc":"2.0","id":"id","result":"got it"}
```

# Errors

To return a JSON-RPC response indicating an error, use
[`Error::new`](https://docs.rs/casper-json-rpc/latest/casper_json_rpc/struct.Error.html#method.new).  Most error
conditions which require returning a reserved error are already handled in the provided warp filters.  The only
exception is
[`ReservedErrorCode::InvalidParams`](https://docs.rs/casper-json-rpc/latest/casper_json_rpc/enum.ReservedErrorCode.html#variant.InvalidParams)
which should be returned by any RPC handler which deems the provided `params: Option<Params>` to be invalid for any
reason.

Generally a set of custom error codes should be provided.  These should all implement
[`ErrorCodeT`](https://docs.rs/casper-json-rpc/latest/casper_json_rpc/trait.ErrorCodeT.html).

## Example custom error code

```rust
use serde::{Deserialize, Serialize};
use casper_json_rpc::ErrorCodeT;

#[derive(Copy, Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[repr(i64)]
pub enum ErrorCode {
    /// The requested item was not found.
    NoSuchItem = -1,
    /// Failed to put the requested item to storage.
    FailedToPutItem = -2,
}

impl From<ErrorCode> for (i64, &'static str) {
    fn from(error_code: ErrorCode) -> Self {
        match error_code {
            ErrorCode::NoSuchItem => (error_code as i64, "No such item"),
            ErrorCode::FailedToPutItem => (error_code as i64, "Failed to put item"),
        }
    }
}

impl ErrorCodeT for ErrorCode {}
```

# License

Licensed under the [Apache License Version 2.0](https://github.com/casper-network/casper-node/blob/master/LICENSE).
