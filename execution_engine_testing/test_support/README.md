# `casper-engine-test-support`

[![LOGO](https://raw.githubusercontent.com/casper-network/casper-node/master/images/casper-association-logo-primary.svg)](https://casper.network/)

[![Build Status](https://drone-auto-casper-network.casperlabs.io/api/badges/casper-network/casper-node/status.svg?branch=dev)](http://drone-auto-casper-network.casperlabs.io/casper-network/casper-node)
[![Crates.io](https://img.shields.io/crates/v/casper-engine-test-support)](https://crates.io/crates/casper-engine-test-support)
[![Documentation](https://docs.rs/casper-engine-test-support/badge.svg)](https://docs.rs/casper-engine-test-support)
[![License](https://img.shields.io/badge/license-Apache-blue)](https://github.com/casper-network/casper-node/blob/master/LICENSE)

A library to support testing of Wasm smart contracts for use on the Casper network.

## `disk_use` binary

`tests/bin/disk_use.rs` houses a binary that will construct global state and profile the disk use of various operations.

It splits the results up into two CSV files:

- `bytes-report-{}.csv` - time-series data over bytes on disk vs number of transfers
- `time-report-{}.csv` - time-series data over time spent vs number of transfers

Put together these two reports can be used to get a relatively quick view into disk and time cost of running transfers and auction processes.

## License

Licensed under the [Apache License Version 2.0](../../LICENSE).
