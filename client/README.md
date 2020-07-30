# casperlabs-client

A client for interacting with the CasperLabs network.

The client can easily be run to interact with a local casperlabs-node.  To run one or more local nodes, see
[the main README](../README.md).

## Running the client

The client runs in one of several modes, each mode performing a single action.  To see all available commands:

```
cd client
cargo run --release -- --help
```

To get further info on any command, run `help` followed by the subcommand, e.g.

```
cargo run --release -- help keygen
```

#### Generate asymmetric signing keys

Some commands require the use of a secret key for signing data.  To generate a secret and public key pair:

```
cargo run --release -- keygen $HOME/.client_keys
```

#### Interacting with a local node

Many client commands require to send HTTP requests and receive responses.  To do this with a local node running on the
same machine, follow the instructions in [the main README](../README.md), ensuring logging is set to at least `info`.

Once the local node starts, the HTTP listening endpoint is printed as an info-level log message.  It can be configured
via the config file for the node (the `http_server.bind_port` option), and if using
[the local node config file](https://github.com/CasperLabs/casperlabs-node/blob/master/resources/local/config.toml), the
endpoint should be `http://localhost:7777`.

For client commands requiring a node address (specified via the `--node-address` or `-n` arg), the default value is
`http://localhost:7777`, and thus can usually be omitted if running a local node.

#### Put a `Deploy`

To create a new `Deploy` and send it to the node for execution and storing:

```
cargo run --release -- put-deploy \
    --secret-key=$HOME/.client_keys/secret_key.pem \
    --standard-payment=100000000 \
    --session-path=<PATH/TO/COMPILED/WASM> \
    --session-arg=seed:U64='12345' \
    --session-arg=my_uref:URef='uref-0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20-007'
```

To see how to format session code args and payment code args:

```
cargo run --release -- put-deploy --show-arg-examples
``` 

On success, the hash identifying the `Deploy` is output as a 64 character hex-encoded string.  The `Deploy` will be
gossiped immediately to all interconnected nodes.

#### Get a `Deploy`

To retrieve that `Deploy` from any node (let's say we have another connected node running with its HTTP server listening
on port 8888):

```
cargo run --release -- get-deploy \
    --node-address=http://localhost:8888 \
    c67eaf71fa9e211aeb448c7f9efd264bcf22a857c223c6b52a4217734167209e
```

#### List stored `Deploy`s

To get a list of all stored `Deploy`s' hashes from any node:

```
cargo run --release -- list-deploys
```
