# Changelog

All notable changes to this project will be documented in this file.  The format is based on [Keep a Changelog].

[comment]: <> (Added:      new features)
[comment]: <> (Changed:    changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed:    now removed features)
[comment]: <> (Fixed:      any bug fixes)
[comment]: <> (Security:   in case of vulnerabilities)



## 2.0.1

### Security
* Implement checks before preprocessing Wasm to avoid potential OOM when initializing table section.
* Implement checks before preprocessing Wasm to avoid references to undeclared functions or globals.


## 2.0.0

### Changed
* Change contract runtime to allow caching global state changes during execution of a single block, also avoiding writing interstitial data to global state.



## 2.0.0 - 2022-05-11

### Changed
* Change contract runtime to allow caching global state changes during execution of a single block, also avoiding writing interstitial data to global state.



## 1.5.0 - 2022-04-05

### Changed
* Temporarily limit the size of individual values stored in global state.

### Security
* `amount` argument is now required for transactions wanting to send tokens using account's main purse. It is now an upper limit on all tokens being transferred within the transaction.
* Significant rework around the responsibilities of the executor, runtime and runtime context objects, with a focus on removing alternate execution paths where unintended escalation of privilege was possible.
* Attenuate the main purse URef to remove WRITE permissions by default when returned via `ret` or passed as a runtime argument.
* Fix a potential panic during Wasm preprocessing.
* `get_era_validators` performs a query rather than execution.



## 1.4.4 - 2021-12-29

### Changed
* No longer checksum-hex encode hash digest and address types.



## 1.4.3 - 2021-12-06

### Changed
* Auction contract now handles minting into an existing purse.
* Default maximum stack size in `WasmConfig` changed to 188.
* Default behavior of LMDB changed to use [`NO_READAHEAD`](https://docs.rs/lmdb/0.8.0/lmdb/struct.EnvironmentFlags.html#associatedconstant.NO_READAHEAD)

### Fixed
* Fix a case where an unlocked and partially unbonded genesis validator with smaller stake incorrectly occupies slot for a non-genesis validator with higher stake.



## [1.4.2] - 2021-11-11

### Changed
* Execution transforms are returned in their insertion order.

### Removed
* Removed `SystemContractCache` as it was not being used anymore

## [1.4.0] - 2021-10-04

### Added
* Added genesis validation step to ensure there are more genesis validators than validator slots.
* Added a support for passing a public key as a `target` argument in native transfers.
* Added a `max_associated_keys` configuration option for a hard limit of associated keys under accounts.

### Changed
* Documented `storage` module and children.
* Reduced visibility to `pub(crate)` in several areas, allowing some dead code to be noticed and pruned.
* Support building and testing using stable Rust.
* Increase price of `create_purse` to 2.5CSPR.
* Increase price of native transfer to 100 million motes (0.1 CSPR).
* Improve doc comments to clarify behavior of the bidding functionality.
* Document `core` and `shared` modules and their children.
* Change parameters to `LmdbEnvironment`'s constructor enabling manual flushing to disk.

### Fixed
* Fix a case where user could potentially supply a refund purse as a payment purse.



## [1.3.0] - 2021-07-19

### Changed
* Update pinned version of Rust to `nightly-2021-06-17`.



## [1.2.0] - 2021-05-27

### Added
* Add validation that the delegated amount of each genesis account is non-zero.
* Add `activate-bid` client contract.
* Add a check in `Mint::transfer` that the source has `Read` permissions.

### Changed
* Change to Apache 2.0 license.
* Remove the strict expectation that minor and patch protocol versions must always increase by 1.

### Removed
* Remove `RootNotFound` error struct.



## [1.1.1] - 2021-04-19

No changes.



## [1.1.0] - 2021-04-13 [YANKED]

No changes.



## [1.0.1] - 2021-04-08

No changes.



## [1.0.0] - 2021-03-30

### Added
* Initial release of execution engine for Casper mainnet.



[Keep a Changelog]: https://keepachangelog.com/en/1.0.0
[unreleased]: https://github.com/casper-network/casper-node/compare/37d561634adf73dab40fffa7f1f1ee47e80bf8a1...dev
[1.4.2]: https://github.com/casper-network/casper-node/compare/v1.4.0...37d561634adf73dab40fffa7f1f1ee47e80bf8a1
[1.4.0]: https://github.com/casper-network/casper-node/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/casper-network/casper-node/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/casper-network/casper-node/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.1.0]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.0.1]: https://github.com/casper-network/casper-node/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/casper-network/casper-node/releases/tag/v1.0.0
