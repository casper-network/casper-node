[![LOGO](images/CasperLabs_Logo_Horizontal_RGB.png)](https://casperlabs.io/)

Casper is the blockchain platform purpose-built to scale opportunity for everyone. Building toward blockchain’s next frontier, Casper is designed for real-world applications without sacrificing usability, cost, decentralization, or security. It removes the barriers that prevent mainstream blockchain adoption by making blockchain friendly to use, open to the world, and future-proof to support innovations today and tomorrow. Guided by open-source principles and built from the ground up to empower individuals, the team seeks to provide an equitable foundation made for long-lasting impact. Read more about our mission at: https://casperlabs.io/company

## Current Development Status
The status on development is reported during the Community calls and is found [here](https://github.com/CasperLabs/Governance/wiki/Current-Status)

The Casper Testnet is live.
- Transactions can be sent to: deploy.casperlabs.io via the client or via
- [Clarity Block Exporer](https://clarity.casperlabs.io)

## Specification

- [Platform Specification](https://techspec.casperlabs.io/en/latest/)
- [Highway Consensus Proofs](https://github.com/CasperLabs/highway/releases/latest)

## Get Started with Smart Contracts
- [Writing Smart Contracts](https://docs.casperlabs.io/en/latest/dapp-dev-guide/index.html)
- [Rust Smart Contract SDK](https://crates.io/crates/cargo-casper)
- [Rust Smart Contract API Docs](https://docs.rs/casper-contract/latest/casper_contract/contract_api/index.html)
- [AssemblyScript Smart Contract API](https://www.npmjs.com/package/@casper/contract)

## Community

- [Discord Server](https://discord.gg/mpZ9AYD)
- [CasperLabs Community Forum](https://forums.casperlabs.io/)
- [Telegram Channel](https://t.me/CasperLabs)

# casper-node

This is the core application for the Casper blockchain.

## Running a validator node

### Setup

Before running a node, prepare your Rust build environment, and build the required system smart contracts:

```
make setup-rs
make build-system-contracts -j
```

### Running one node

To run a validator node you will need to specify a config file.  For example, this is one with
[local configuration options](resources/local/config.toml).

Note that all paths specified in the config file must be absolute paths or relative to the config file itself.

It is possible to specify individual config file options from the command line using one or more args in the form of
`-C=<SECTION>.<KEY>=<VALUE>`.  These will override values set in a config file if provided, or will override the
default values otherwise.  For example:

```
cargo run --release -- validator resources/local/config.toml -C=consensus.secret_key_path=secret_keys/node-1.pem
```

Beware that no semicolons (`;`) can be passed in any string on the command-line, as these serve as top-level argument separators when passing arguments via environment variables.

Note that `network.known_addresses` must refer to public listening addresses of one or more
currently-running nodes.  If the node cannot connect to any of these addresses, it will panic.  The
node _can_ be run with this referring to its own address, but it will be equivalent to specifying an
empty list for `known_addresses` - i.e. the node will run and listen, but will be reliant on other
nodes connecting to it in order to join the network.  This would be normal for the very first node
of a network, but all subsequent nodes should normally specify that first node's public listening
address as their `known_addresses`.

As well as the commented [local config file](resources/local/config.toml), there is a commented
[local chainspec](resources/local/chainspec.toml) in the same folder.

### Running multiple nodes on one machine

If you want to run multiple instances on the same machine, you will need to modify the following values from the
[local configuration options](resources/local/config.toml):

* the secret key path should be different for each node, e.g. `-C=consensus.secret_key_path=secret_keys/node-2.pem`
* the storage path should be different for each node, e.g. `-C=storage.path=/tmp/node-2-storage`
* the bind port should be different for each node, but if you set it to 0, the node will automatically be assigned a
random port, e.g. `-C=network.bind_port=0`

The nodes can take quite a long time to become fully interconnected.  This is dependent on the `network.gossip_interval`
value (in milliseconds).  Nodes gossip their own listening addresses at this frequency.  To reduce the initial time to
become fully interconnected, this value can be reduced, e.g. `-C=network.gossip_interval=1000` for once every second.
However, beware that this will also increase the network traffic, as gossip rounds between the nodes will continue to
be exchanged at this frequency for the duration of the network.

There is a [shell script](run-dev.sh) which automates the process of running multiple nodes on a single machine.

Note that running multiple nodes on a single machine is normally only recommended for test purposes.

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
