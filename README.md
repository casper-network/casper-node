[![LOGO](images/CasperLabs_Logo_Horizontal_RGB.png)](https://casperlabs.io/)

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
- [Rust Smart Contract API Docs](https://docs.rs/casperlabs-contract/latest/casperlabs_contract/contract_api/index.html)
- [AssemblyScript Smart Contract API](https://www.npmjs.com/package/@casperlabs/contract)

## Community

- [Discord Server](https://discord.gg/mpZ9AYD)
- [CasperLabs Community Forum](https://forums.casperlabs.io/)
- [Telegram Channel](https://t.me/CasperLabs)

# casperlabs-node

This is the core application for the CasperLabs blockchain.

## Running a validator node

### Setup

Before running a node, prepare your Rust build environment, and build the required system smart contracts:

```
make setup-rs
make build-system-contracts -j
```

### Running

To run a validator node you will need to specify a config file.  For example, this is one with
[local configuration options](resources/local/config.toml).

Note that all paths specified in the config file must be absolute paths or relative to the config file itself.

It is possible to specify individual config file options from the command line using one or more args in the form of
`-C=<SECTION>.<KEY>=<VALUE>`.  These will override values set in a config file if provided, or will override the
default values otherwise.  For example:

```
cargo run --release -- validator -c=resources/local/config.toml -C=consensus.secret_key_path=secret_keys/node-1.pem
```

To create a config file for editing, you can either copy the example config file linked above, or you can generate a
default one:

```
cargo run --release -- generate-config > config.toml
# ... edit config.toml
cargo run --release -- validator -c=config.toml
```

**NOTE:** If you want to run multiple instances on the same machine, ensure you modify the `[storage.path]` field of
their configuration files to give each a unique path, or else instances will share database files.

As well as the commented [local config file](resources/local/config.toml), there is a commented
[local chainspec](resources/local/chainspec.toml) in the same folder.

## Logging

Logging can be enabled by setting the environment variable `RUST_LOG`.  This can be set to one of the following levels,
from lowest priority to highest: `trace`, `debug`, `info`, `warn`, `error`:

```
RUST_LOG=info cargo run --release -- validator -c=resources/local/config.toml
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

See [the client README](client/README.md).
