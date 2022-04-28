# Changelog

All notable changes to this project will be documented in this file.  The format is based on [Keep a Changelog].

[comment]: <> (Added:      new features)
[comment]: <> (Changed:    changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed:    now removed features)
[comment]: <> (Fixed:      any bug fixes)
[comment]: <> (Security:   in case of vulnerabilities)



## [Unreleased]

### Added
* Added `WasmTestBuilder::get_execution_journals` method for returning execution journals for all test runs.
* Added `WasmTestBuilder::advance_era`, `WasmTestBuilder::advance_eras_by`, and `WasmTestBuilder::advance_eras_by_default_auction_delay` to advance chain and run auction contract in test environment.

### Changed
* `WasmTestBuilder::get_transforms` is deprecated in favor of `WasmTestBuilder::get_execution_journals`.
* `deploy_hash` field is now defaulted to a random value rather than zeros in `DeployItemBuilder`.



<<<<<<< HEAD
=======
## 2.1.0

### Added
* Add further helper methods to `WasmTestBuilder`.



>>>>>>> upstream/dev
## 2.0.3 - 2021-12-06

### Added
* Added `WasmTestBuilder::get_balance_keys` function.



## 2.0.2 - 2021-11-24

### Changed
* Revert the change to the path detection logic applied in v2.0.1.



## [2.0.1] - 2021-11-4

### Changed
* Change the path detection logic for compiled Wasm as used by the casper-node monorepo.

### Deprecated
* Deprecate the `test-support` feature.  It had and continues to have no effect when enabled.



## [2.0.0] - 2021-11-01

### Added
* Provide fine-grained support for testing all aspects of smart contract execution, including:
    * `WasmTestBuilder` for building and running a test to exercise a smart contract
    * `DeployItemBuilder` for building a `DeployItem` from a smart contract
    * `ExecuteRequestBuilder` for building an `ExecuteRequest` to execute a given smart contract
    * `AdditiveMapDiff` to allow easy comparison of two AdditiveMaps
    * `StepRequestBuilder` for building a `StepRequest` (generally only used by the execution engine itself)
    * `UpgradeRequestBuilder` for building an `UpgradeRequest` (generally only used by the execution engine itself)
* Provide `InMemoryWasmTestBuilder` which will be suitable in most cases for testing a smart contract
* Provide `LmdbWasmTestBuilder` can be used where global state needs to be persisted after execution of a smart contract
* Provide several helper functions in `utils` module
* Provide several default consts and statics useful across many test scenarios

### Removed
* Remove coarse-grained support and newtypes for testing smart contracts, including removal of:
    * `Account`
    * `AccountHash`
    * `Error`
    * `Session`
    * `SessionBuilder`
    * `SessionTransferInfo`
    * `TestContext`
    * `TestContextBuilder`
    * `Value`



## [1.4.0] - 2021-10-04

### Changed
* Support building and testing using stable Rust.



## [1.3.0] - 2021-07-19

### Changed
* Update pinned version of Rust to `nightly-2021-06-17`.



## [1.2.0] - 2021-05-28

### Changed
* Change to Apache 2.0 license.



## [1.1.1] - 2021-04-19

No changes.



## [1.1.0] - 2021-04-13 [YANKED]

No changes.



## [1.0.1] - 2021-04-08

No changes.



## [1.0.0] - 2021-03-30

### Added
* Initial release of execution-engine test support framework compatible with Casper mainnet.



[Keep a Changelog]: https://keepachangelog.com/en/1.0.0
[unreleased]: https://github.com/casper-network/casper-node/compare/04f48a467...dev
[2.0.1]: https://github.com/casper-network/casper-node/compare/13585abcf...04f48a467
[2.0.0]: https://github.com/casper-network/casper-node/compare/v1.4.0...13585abcf
[1.4.0]: https://github.com/casper-network/casper-node/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/casper-network/casper-node/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/casper-network/casper-node/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.1.0]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.0.1]: https://github.com/casper-network/casper-node/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/casper-network/casper-node/releases/tag/v1.0.0
