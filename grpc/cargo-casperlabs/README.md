# `cargo casperlabs`

[![LOGO](https://raw.githubusercontent.com/CasperLabs/CasperLabs/master/CasperLabs_Logo_Horizontal_RGB.png)](https://casperlabs.io/)

[![Build Status](https://drone-auto.casperlabs.io/api/badges/CasperLabs/CasperLabs/status.svg?branch=dev)](http://drone-auto.casperlabs.io/CasperLabs/CasperLabs)
[![Crates.io](https://img.shields.io/crates/v/cargo-casperlabs)](https://crates.io/crates/cargo-casperlabs)
[![Documentation](https://docs.rs/cargo-casperlabs/badge.svg)](https://docs.rs/cargo-casperlabs)
[![License](https://img.shields.io/badge/license-COSL-blue.svg)](https://github.com/CasperLabs/CasperLabs/blob/master/LICENSE)

A command line tool for creating a Wasm smart contract and tests for use on the CasperLabs network.

## Installation

`cargo casperlabs` is a Cargo subcommand which can be installed via `cargo install`:

```
cargo install cargo-casperlabs
```

To install from the latest `dev` branch:

```
git clone https://github.com/CasperLabs/CasperLabs
cd CasperLabs/execution-engine/cargo-casperlabs
cargo install cargo-casperlabs --path=.
```

## Usage

To create a folder "my_project" containing an example contract and a separate test crate for the contract:

```
cargo casperlabs my_project
```

This creates the following files:

```
my_project/
├── contract
│   ├── .cargo
│   │   └── config
│   ├── Cargo.toml
│   ├── rust-toolchain
│   └── src
│       └── main.rs
├── tests
│   ├── build.rs
│   ├── Cargo.toml
│   ├── rust-toolchain
│   ├── src
│   │   └── integration_tests.rs
│   └── wasm
│       ├── mint_install.wasm
│       ├── pos_install.wasm
│       ├── standard_payment_install.wasm
│       └── standard_payment.wasm
└── .travis.yml
```

### Building the contract

To build the contract, the correct version of Rust must be installed along with the Wasm target:

```
cd my_project/contract
rustup install $(cat rust-toolchain)
rustup target add --toolchain=$(cat rust-toolchain) wasm32-unknown-unknown
```

The contract can now be built using:

```
cargo build --release
```

and will be built to `my_project/contract/target/wasm32-unknown-unknown/release/contract.wasm`.

### Testing the contract

Running the test will automatically build the contract in release mode, copy it to the "tests/wasm" folder, then build
and run the test:

```
cd my_project/tests
cargo test
```

## License

Licensed under the [CasperLabs Open Source License (COSL)](https://github.com/CasperLabs/CasperLabs/blob/master/LICENSE).
