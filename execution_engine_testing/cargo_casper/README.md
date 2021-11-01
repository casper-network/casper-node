# `cargo-casper`

[![LOGO](https://raw.githubusercontent.com/casper-network/casper-node/master/images/casper-association-logo-primary.svg)](https://casper.network/)

[![Build Status](https://drone-auto-casper-network.casperlabs.io/api/badges/casper-network/casper-node/status.svg?branch=dev)](http://drone-auto-casper-network.casperlabs.io/casper-network/casper-node)
[![Crates.io](https://img.shields.io/crates/v/cargo-casper)](https://crates.io/crates/cargo-casper)
[![License](https://img.shields.io/badge/license-Apache-blue)](https://github.com/casper-network/casper-node/blob/dev/LICENSE)

A command line tool for creating a Wasm smart contract and tests for use on the Casper network.

---

## Installation

`cargo casper` is a Cargo subcommand which can be installed via `cargo install`:

```
cargo install cargo-casper
```

To install from the latest `dev` branch:

```
git clone https://github.com/casper-network/casper-node
cd casper-node/execution_engine_testing/cargo_casper
cargo install cargo-casper --path=.
```

## Usage

To create a folder "my_project" containing a basic example contract and a separate test crate for the contract:

```
cargo casper my_project
```

This creates the following files:

```
my_project/
├── contract
│   ├── .cargo
│   │   └── config.toml
│   ├── Cargo.toml
│   └── src
│       └── main.rs
├── Makefile
├── rust-toolchain
├── tests
│   ├── Cargo.toml
│   └── src
│       └── integration_tests.rs
└── .travis.yml
```

### Building the contract

To build the contract, the correct version of Rust must be installed along with the Wasm target:

```
cd my_project
make prepare
```

The contract can now be built using:

```
make build-contract
```

and will be built to `my_project/contract/target/wasm32-unknown-unknown/release/contract.wasm`.

### Testing the contract

Running the test will automatically build the contract in release mode, copy it to the "tests/wasm" folder, then build
and run the test:

```
make test
```

## License

Licensed under the [Apache License Version 2.0](https://github.com/casper-network/casper-node/blob/master/LICENSE).
