[![LOGO](images/CasperLabs_Logo_Horizontal_RGB.png)](https://casperlabs.io/)

Casper is the blockchain platform purpose-built to scale opportunity for everyone. Building toward blockchain’s next frontier, Casper is designed for real-world applications without sacrificing usability, cost, decentralization, or security. It removes the barriers that prevent mainstream blockchain adoption by making blockchain friendly to use, open to the world, and future-proof to support innovations today and tomorrow. Guided by open-source principles and built from the ground up to empower individuals, the team seeks to provide an equitable foundation made for long-lasting impact. Read more about our mission at: https://casperlabs.io/company

## Current Development Status
The status on development is reported during the Community calls and is found [here](https://github.com/CasperLabs/Governance/wiki/Current-Status)

The Casper Testnet is live.
- Transactions can be sent to: deploy.casperlabs.io via the client or via Clarity.
- [Clarity Block Exporer](https://clarity.casperlabs.io)

## Specification

- [Platform Specification](https://techspec.casperlabs.io/en/latest/)
- [Highway Consensus Proofs](https://github.com/CasperLabs/highway/releases/latest)

## Get Started with Smart Contracts
- [Writing Smart Contracts](https://docs.casperlabs.io/en/latest/dapp-dev-guide/index.html)
- [Rust Smart Contract SDK](https://crates.io/crates/cargo-casper)
- [Rust Smart Contract API Docs](https://docs.rs/casperlabs-contract/0.6.1/casperlabs_contract/contract_api/index.html)
- [AssemblyScript Smart Contract API](https://www.npmjs.com/package/@casperlabs/contract)

## Community

- [Discord Server](https://discord.gg/mpZ9AYD)
- [CasperLabs Community Forum](https://forums.casperlabs.io/)
- [Telegram Channel](https://t.me/CasperLabs)

# casper-node

This is the core application for the Casper blockchain.

## Running a validator node from Source

### Pre-Requisites for Building

cmake 3.1.4 or greater

[Rust](https://www.rust-lang.org/tools/install)

libssl-dev

pkg-config

gcc

g++

### Setup

Before building a node, prepare your Rust build environment, and build the required system smart contracts:

```
make setup-rs
make build-system-contracts -j
```

The node software can be compiled afterwards:

```
cargo build -p casper-node --release
```

The result will be a `casper-node` binary found in `target/release`.  Copy this somewhere into your
PATH, or substitute `target/release/casper-node` for `casper-node` in all examples below.

### Running one node

To run a validator node you will need to specify a config file and launch the validator subcommand, for example

```
casper-node validator /etc/casper-node/config.toml
```

The node ships with an [example configuration file](resources/local/config.toml) that should be setup first.  There is
also a commented [local chainspec](resources/local/chainspec.toml) in the same folder.

For launching, the following configuration values must be properly set:

| Setting                   | Description |
| :-------------------------| :---------- |
| `network.known_addresses` | Must refer to public listening addresses of one or more currently-running nodes.  If the node cannot connect to any of these addresses, it will panic.  The node _can_ be run with this referring to its own address, but it will be equivalent to specifying an empty list for `known_addresses` - i.e. the node will run and listen, but will be reliant on other nodes connecting to it in order to join the network.  This would be normal for the very first node of a network, but all subsequent nodes should normally specify that first  node's public listening address as their `known_addresses`. |


### Running multiple nodes on one machine

If you want to run multiple instances on the same machine, you will need to modify the following
configuration values:

| Setting                     | Description |
| :---------------------------| :---------- |
| `consensus.secret_key_path` | The path to the secret key must be different for each node, as no two nodes should be using an identical secret key. |
| `storage.path`              | Storage must be separate for each node, e.g. `/tmp/node-2-storage` for the second node |
| `network.bind_address`      | Each node requires a different bind address, although the port can be set to `0`, which will cause the node to select a random port. |
| `network.gossip_interval`   | (optional) To reduce the initial time to become fully interconnected, this value can be reduced, e.g. set  to `1000` for once every second.  However, beware thatthis will also increase the network traffic, as gossip rounds between the nodes will continue to be exchanged at this frequency for the duration of the network. |

The nodes can take quite a long time to become fully interconnected.  This is dependent on the
`network.gossip_interval` value (in milliseconds).  Nodes gossip their own listening addresses at
this frequency.

There is a [tool](https://github.com/CasperLabs/casper-node/tree/master/utils/nctl) which automates the process of running multiple nodes on a single machine.

Note that running multiple nodes on a single machine is normally only recommended for test purposes.

## Configuration

In general nodes are configured through a configuration file, typically named `config.toml`.  This
file may reference other files or locations through relative paths.  When it does, note that all
paths that are not absolute will be resolved relative to `config.toml` directory.

### CLI overrides

It is possible to override config file options from the command line using one or more args in the
form of `-C=<SECTION>.<KEY>=<VALUE>`.  These will override values set in a config file if provided,
or will override the default values otherwise.  For example

```
casper-node validator /etc/casper-node/config.toml \
    -C=consensus.secret_key_path=secret_keys/node-1.pem \
    -C=network.known_addresses="[1.2.3.4:34553, 200.201.203.204:34553]"
```

will override the `consensus.secret_key_path` and the `network.known_addresses` configuration
setting.

Be aware that semicolons are prohibited (even escaped) from being used in any option passed on the
command line.

### Environment overrides

Some environments may call for overriding options through the environment rather than the command line.  In this
scenario, the `NODE_CONFIG` environment variable can be used. For example, the command from the previous section can be
alternatively expressed as

```
export NODE_CONFIG=consensus.secret_key_path=secret_keys/node-1.pem;network.known_addresses=[1.2.3.4:34553, 200.201.203.204:34553]
casper-node validator /etc/casper-node/config.toml
```

Note how the semicolon is used to separate configuration overrides here.

### Development environment variables

To set the threshold at which a warn-level log message is generated for a long-running reactor event, use the env var
`CL_EVENT_MAX_MICROSECS`.  For example, to set the threshold to 1 millisecond:

```
CL_EVENT_MAX_MICROSECS=1000
```

## Logging

Logging can be enabled by setting the environment variable `RUST_LOG`.  This can be set to one of the following levels,
from lowest priority to highest: `trace`, `debug`, `info`, `warn`, `error`:

```
RUST_LOG=info cargo run --release -- validator resources/local/config.toml
```

If the environment variable is unset, it is equivalent to setting `RUST_LOG=error`.

#### Log message format

A typical log message will look like:

```
Jun 09 01:40:17.315 INFO  [casper_node::components::api_server api_server.rs:127] starting HTTP server; server_addr=127.0.0.1:7777
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
RUST_LOG=casper_node::components::small=trace,casper_node::comp=info,warn
```

## Running a client

See [the client README](client/README.md).

## Running a local network

See [the nctl utility README](utils/nctl/README.md).
