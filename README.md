[![LOGO](CasperLabs_Logo_Horizontal_RGB.png)](https://casperlabs.io/)

Casper is the blockchain platform purpose-built to scale opportunity for everyone. Building toward blockchain’s next frontier, Casper is designed for real-world applications without sacrificing usability, cost, decentralization, or security. It removes the barriers that prevent mainstream blockchain adoption by making blockchain friendly to use, open to the world, and future-proof to support innovations today and tomorrow. Guided by open-source principles and built from the ground up to empower individuals, the team seeks to provide an equitable foundation made for long-lasting impact. Read more about our mission at: https://casperlabs.io/company

## Current Development Status
The status on development is reported during the Community calls and is found [here](https://github.com/CasperLabs/Governance/wiki/Current-Status)

The CasperLabs Testnet is live.  
- Transactions can be sent to: deploy.casperlabs.io via the client or via 
- [Clarity Block Exporer](https://clarity.casperlabs.io)

## Specification

- [Platform Specification](https://techspec.casperlabs.io/en/latest/)
- [Highway Consensus Proofs](https://github.com/CasperLabs/highway/releases/latest)

## Get Started with Smart Contracts
- [Writing Smart Contracts](https://docs.casperlabs.io/en/latest/dapp-dev-guide/index.html)
- [Rust Smart Contract SDK](https://crates.io/crates/cargo-casperlabs)
- [Rust Smart Contract API Docs (v0.5.1)](https://docs.rs/casperlabs-contract/0.5.1/casperlabs_contract/contract_api/index.html)
- [Assemblyscript Smart Contract API (https://www.npmjs.com/package/@casperlabs/contract)

## Community

- [Discord Server](https://discord.gg/mpZ9AYD)
- [CasperLabs Community Forum](https://forums.casperlabs.io/)
- [Telegram Channel](https://t.me/CasperLabs)

# casperlabs-node

This is the core application for the CasperLabs blockchain.

## Running a validator node

To run a validator node with the default configuration:

```
cargo run --release -- validator
```

It is very likely that the configuration requires editing though, so typically one will want to generate a configuration
file first, edit it and then run:

```
cargo run --release -- generate-config > mynode.toml
# ... edit mynode.toml
cargo run --release -- validator --config=mynode.toml
```

**NOTE:** If you want to run multiple instances on the same machine, ensure you modify the `[storage.path]` field of
their configuration files to give each a unique path, or else instances will share database files.

## Logging

Logging can be enabled by setting the environment variable `RUST_LOG`.  This can be set to one of the following levels,
from lowest priority to highest: `trace`, `debug`, `info`, `warn`, `error`:

```
RUST_LOG=info cargo run --release -- validator
```

If the environment variable is unset, it is equivalent to setting `RUST_LOG=error`.

#### Log message format

A typical log message will look like:

```
Jun 09 01:40:17.315 INFO  [casperlabs_node::components::api_server api_server.rs:127] starting HTTP server; server_addr=127.0.0.1:7777
```

This is comprised of the following parts:
* timestamp
* log level
* full module path (not to be confused with filesystem path) of the source of the message
* filename and line number of the source of the message
* message

#### Filtering log messages

`RUST_LOG` can be set to enable varying levels for different modules.  Simply set it to a comma-separated list of
`module-path=level`, where the module path is as shown above in the typical log message, with the end truncated to suit.

For example, to enable `trace` level logging for the `small_network` module in `components`, `info` level for all other
modules in `components`, and `warn` level for the remaining codebase:

```
RUST_LOG=casperlabs_node::components::small=trace,casperlabs_node::comp=info,warn
```

## Running a client

There is a minimal client which can be executed to store and retrieve `Deploy`s on validator nodes.

The client targets the HTTP service of a validator node.  This can be configured via the config file for the node, and
the actual bound endpoint is displayed as an info-level log message just after node startup.

#### Put a `Deploy`

To create a new random `Deploy` and store it:

```
cargo run --release --bin=casperlabs-client -- put-deploy http://localhost:7777
```

On success, the hash identifying the `Deploy` is output as a 64 character hex-encoded string.  The `Deploy` will be
gossiped immediately to all interconnected validator nodes.

#### Get a `Deploy`

To retrieve that deploy from any node:

```
cargo run --release --bin=casperlabs-client -- get-deploy http://localhost:8888 a555b68c8fed43078db6022a3de83fce97c1d80caf070c3654f9526d149e8182
```

#### List stored `Deploy`s

To get a list of all stored `Deploy`s' hashes:

```
cargo run --release --bin=casperlabs-client -- list-deploys http://localhost:9999
```
