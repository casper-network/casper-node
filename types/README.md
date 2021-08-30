# `casper-types`

[![LOGO](https://raw.githubusercontent.com/casper-network/casper-node/master/images/casper-association-logo-primary.svg)](https://casper.network/)

[![Build Status](https://drone-auto-casper-network.casperlabs.io/api/badges/casper-network/casper-node/status.svg?branch=dev)](http://drone-auto-casper-network.casperlabs.io/casper-network/casper-node)
[![Crates.io](https://img.shields.io/crates/v/casper-types)](https://crates.io/crates/casper-types)
[![Documentation](https://docs.rs/casper-types/badge.svg)](https://docs.rs/casper-types)
[![License](https://img.shields.io/badge/license-Apache-blue)](https://github.com/CasperLabs/casper-node/blob/master/LICENSE)

## `no_std`

By default, the `no-std` feature is enabled which in turn enables a similar feature on many dependent crates.  To use
the library in a `std` environment, disable the default features and enable the `std` feature.  For example:

```toml
casper-types = { version = "1.0.0", default-features = false, features = ["std"] }
```

## License

Licensed under the [Apache License Version 2.0](https://github.com/casper-network/casper-node/blob/master/LICENSE).
