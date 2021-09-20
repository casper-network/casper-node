# `casper-types`

[![LOGO](https://raw.githubusercontent.com/casper-network/casper-node/master/images/casper-association-logo-primary.svg)](https://casper.network/)

[![Build Status](https://drone-auto-casper-network.casperlabs.io/api/badges/casper-network/casper-node/status.svg?branch=dev)](http://drone-auto-casper-network.casperlabs.io/casper-network/casper-node)
[![Crates.io](https://img.shields.io/crates/v/casper-types)](https://crates.io/crates/casper-types)
[![Documentation](https://docs.rs/casper-types/badge.svg)](https://docs.rs/casper-types)
[![License](https://img.shields.io/badge/license-Apache-blue)](https://github.com/CasperLabs/casper-node/blob/master/LICENSE)

Types shared by many casper crates for use on the Casper network.

## `no_std`

The crate is `no_std` (using the `core` and `alloc` crates) unless any of the following features are enabled:

* `json-schema` to enable many types to be used to produce JSON-schema data via the [`schemars`](https://crates.io/crates/schemars) crate
* `datasize` to enable many types to derive the [`DataSize`](https://github.com/casperlabs/datasize-rs) trait
* `gens` to enable many types to be produced in accordance with [`proptest`](https://crates.io/crates/proptest) usage for consumption within dependee crates' property testing suites

## License

Licensed under the [Apache License Version 2.0](https://github.com/casper-network/casper-node/blob/master/LICENSE).
