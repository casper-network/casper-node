# `casper-types`

[![LOGO](https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Horizontal_RGB.png)](https://casperlabs.io/)

[![Build Status](https://drone-auto.casperlabs.io/api/badges/CasperLabs/casper-node/status.svg?branch=master)](http://drone-auto.casperlabs.io/CasperLabs/casper-node)
[![Crates.io](https://img.shields.io/crates/v/casper-types)](https://crates.io/crates/casper-types)
[![Documentation](https://docs.rs/casper-types/badge.svg)](https://docs.rs/casper-types)
[![License](https://img.shields.io/badge/license-COSL-blue.svg)](https://github.com/CasperLabs/casper-node/blob/master/LICENSE)

Types used by nodes and clients on the Casper network.

## `no_std`

By default, the `no-std` feature is enabled which in turn enables a similar feature on many dependent crates.  To use
the library in a `std` environment, disable the default features and enable the `std` feature.  For example:

```toml
casper-types = { version = "1.0.0", default-features = false, features = ["std"] }
```

## License

Licensed under the [CasperLabs Open Source License (COSL)](https://github.com/CasperLabs/casper-node/blob/master/LICENSE).
