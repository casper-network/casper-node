[![LOGO](https://raw.githubusercontent.com/casper-network/casper-node/master/images/casper-association-logo-new.svg)](https://casper.network/)

# casper-node

This is the core application for the Casper blockchain.

## Casper Blockchain

Casper is the blockchain platform purpose-built to scale opportunity for everyone. Building toward blockchain’s next frontier,
Casper is designed for real-world applications without sacrificing usability, cost, decentralization, or security. It removes
the barriers that prevent mainstream blockchain adoption by making blockchain friendly to use, open to the world, and
future-proof to support innovations today and tomorrow. Guided by open-source principles and built from the ground up to
empower individuals, the team seeks to provide an equitable foundation made for long-lasting impact. Read more about our
mission at: https://casper.network/network/casper-association

### Current Development Status
The status on development is reported during the Community calls and is found [here](https://github.com/CasperLabs/Governance/wiki/Current-Status)

The Casper MainNet is live.
- [cspr.live Block Explorer](https://cspr.live)

### Specification

- [Platform Specification](https://docs.casperlabs.io/design/)
- [Highway Consensus Proofs](https://github.com/CasperLabs/highway/releases/latest)

### Get Started with Smart Contracts
- [Writing Smart Contracts](https://docs.casperlabs.io/developers/)
- [Rust Smart Contract SDK](https://crates.io/crates/cargo-casper)
- [Rust Smart Contract API Docs](https://docs.rs/casper-contract/latest/casper_contract/contract_api/index.html)
- [AssemblyScript Smart Contract API](https://www.npmjs.com/package/casper-contract)

### Community

- [Discord Server](https://discord.gg/mpZ9AYD)
- [Telegram Channel](https://t.me/casperofficialann)



## Running a casper-node from source

### Pre-Requisites for Building

* CMake 3.1.4 or greater
* [Rust](https://www.rust-lang.org/tools/install)
* libssl-dev
* pkg-config
* gcc
* g++
* recommended [wasm-strip](https://github.com/WebAssembly/wabt) (used to reduce the size of compiled Wasm)

```sh
# Ubuntu prerequisites setup example
apt update
apt install cmake libssl-dev pkg-config gcc g++ -y
# the '-s -- -y' part ensures silent mode. Omit if you want to customize
curl https://sh.rustup.rs -sSf | sh -s -- -y
```


### Setup

Before building a node, prepare your Rust build environment:

```
make setup-rs
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
also a template for a local [chainspec](resources/local/chainspec.toml.in) in the same folder.

For launching, the following configuration values must be properly set:

| Setting                   | Description |
| :-------------------------| :---------- |
| `network.known_addresses` | Must refer to public listening addresses of one or more currently-running nodes.  If the node cannot connect to any of these addresses, it will panic.  The node _can_ be run with this referring to its own address, but it will be equivalent to specifying an empty list for `known_addresses` - i.e. the node will run and listen, but will be reliant on other nodes connecting to it in order to join the network.  This would be normal for the very first node of a network, but all subsequent nodes should normally specify that first  node's public listening address as their `known_addresses`. |

__The node will not run properly without another node to connect to.  It is recommended that multiple nodes are run.__

### Running multiple nodes on one machine

There is a [tool](https://github.com/casper-network/casper-node/tree/dev/utils/nctl) which automates the process of running multiple nodes on a single machine.

Note that running multiple nodes on a single machine is normally only recommended for test purposes.

## Configuration

In general nodes are configured through a configuration file, typically named `config.toml`.  This
file may reference other files or locations through relative paths.  When it does, note that all
paths that are not absolute will be resolved relative to `config.toml` directory.


### Environment overrides

Some environments may call for overriding options through the environment.  In this
scenario, the `NODE_CONFIG` environment variable can be used. For example:
alternatively expressed as

```
export NODE_CONFIG=consensus.secret_key_path=secret_keys/node-1.pem;network.known_addresses=[1.2.3.4:34553, 200.201.203.204:34553]
casper-node validator /etc/casper-node/config.toml
```

Note how the semicolon is used to separate configuration overrides here.

### Other environment variables

To set the threshold at which a warn-level log message is generated for a long-running reactor event, use the env var
`CL_EVENT_MAX_MICROSECS`.  For example, to set the threshold to 1 millisecond:

```
CL_EVENT_MAX_MICROSECS=1000
```

To set the threshold above which the size of the current scheduler queues will be dumped to logs, use the `CL_EVENT_QUEUE_DUMP_THRESHOLD` variable. For example, to set the threshold to 10000 events:

```
CL_EVENT_QUEUE_DUMP_THRESHOLD=10000
```

This will dump a line to the log if the total number of events in queues exceeds 10000. After each dump, the threshold will be automatically increased by 10% to avoid log flooding.

Example log entry:
```
Current event queue size (11000) is above the threshold (10000): details [("FinalitySignature", 3000), ("FromStorage", 1000), ("NetworkIncoming", 6500), ("Regular", 500)]
```

## Logging

Logging can be enabled by setting the environment variable `RUST_LOG`.  This can be set to one of the following levels,
from lowest priority to highest: `trace`, `debug`, `info`, `warn`, `error`:

```
RUST_LOG=info cargo run --release -- validator resources/local/config.toml
```

If the environment variable is unset, it is equivalent to setting `RUST_LOG=error`.

### Log message format

A typical log message will look like:

```
Jun 09 01:40:17.315 INFO  [casper_node::components::rpc_server rpc_server.rs:127] starting HTTP server; server_addr=127.0.0.1:7777
```

This is comprised of the following parts:
* timestamp
* log level
* full module path (not to be confused with filesystem path) of the source of the message
* filename and line number of the source of the message
* message

### Filtering log messages

`RUST_LOG` can be set to enable varying levels for different modules.  Simply set it to a comma-separated list of
`module-path=level`, where the module path is as shown above in the typical log message, with the end truncated to suit.

For example, to enable `trace` level logging for the `network` module in `components`, `info` level for all other
modules in `components`, and `warn` level for the remaining codebase:

```
RUST_LOG=casper_node::components::network=trace,casper_node::comp=info,warn
```

### Logging network messages and tracing events

Special logging targets exist in `net_in` and `net_out` which can be used to log every single network message leaving or
entering a node when set to trace level:

```
RUST_LOG=net_in::TRACE,net_out::TRACE
```

All messages in these logs are also assigned a unique ID that is different even if the same message is sent to multiple
nodes. The receiving node will log them using the same ID as the sender, thus enabling the tracing of a message across
multiple nodes provided all logs are available.

Another helpful logging feature is ancestor logging. If the target `dispatch` is set to at least debug level, events
being dispatched will be logged as well. Any event has an id (`ev`) and may have an ancestor (`a`), which is the previous
event whose effects caused the resulting event to be scheduled. As an example, if an incoming network message gets
asssigned an ID of `ev=123`, the first round of subsequent events will show `a=123` as their ancestor in the logs.

### Changing the logging filter at runtime

If necessary, the filter of a running node can be changed using the diagnostics port, using the `set-log-filter`
command. See the "Diagnostics port" section for details on how to access it.

## Debugging

Some additional debug functionality is available, mainly allowed for inspections of the internal event queue.

### Diagnostics port

If the configuration option `diagnostics_port.enabled` is set to `true`, a unix socket named `debug.socket` by default can be found next to the configuration while the node is running.

#### Interactive use

The `debug.socket` can be connected to by tools like `socat` for interactive use:

```sh
socat readline unix:/path/to/debug.socket
```

Entering `help` will show available commands. The `set` command allows configuring the current connection, see `set --help`.

#### Example: Collecting a consensus dump

After connecting using `socat` (see above), we set the output format to JSON:

```
set --output=json
```

A confirmation will acknowledge the settings change (unless `--quiet=true` is set):

```
{
  "Success": {
    "msg": "session unchanged"
  }
}
```

We can now call `dump-consensus` to get the _latest_ era serialized in JSON format:

```
dump-consensus
{
  "Success": {
    "msg": "dumping consensus state"
  }
}
{"id":8,"start_time":"2022-03-01T14:54:42.176Z","start_height":88,"new_faulty" ...
```

An era other than the latest can be dumped by specifying as a parameter, _e.g._ `dump-consensus 3` will dump the third era. See `dump-consensus --help` for details.

#### Example: Dumping the event queue

With the connection set to JSON output (see previous example), we can also dump the event queues:

```
dump-queues
{
  "Success": {
    "msg": "dumping queues"
  }
}
{"queues":{"Regular":[],"Api":[],"Network":[],"Control":[],"NetworkIncoming":[]
}}{"queues":{"Api":[],"Regular":[],"Control":[],"NetworkIncoming":[],"Network":
[]}}{"queues":{"Network":[],"Control":[],"Api":[],"NetworkIncoming":[],"Regular
":[]}}
```

Empty output will be produced on a node that is working without external pressure, as the queues will be empty most of the time.


#### Non-interactive use

The diagnostics port can also be scripted by sending a newline-terminated list of commands through `socat`. For example, the following sequence of commands will collect a consensus dump without the success-indicating header:

```
set -o json -q true
dump-consensus
```

For ad-hoc dumps, this can be shortened and piped into `socat`:

```sh
echo -e 'set -o json -q true\ndump-consensus' | socat - unix-client:debug.socket > consensus-dump.json
```

This results in the latest era being dumped into `consensus-dump.json`.


## Running a client

See [the client README](https://github.com/casper-ecosystem/casper-client-rs#readme).

## Running a local network

See [the nctl utility README](utils/nctl/README.md).

## Running on an existing network

To support upgrades with a network, the casper-node is installed using scripts distributed with the
[casper-node-launcher](https://github.com/casper-network/casper-node-launcher).
