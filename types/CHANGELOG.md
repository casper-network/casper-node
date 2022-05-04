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
* Add new `bytesrepr::Error::NotRepresentable` error variant that represents values that are not representable by the serialization format.
* Add new `Key::ChainspecRegistry` key variant under which the `ChainspecRegistry` is written.
* Add a new type `WithdrawPurses` which is meant to represent `UnbondingPurses` as they exist in current live networks.
* Extend asymmetric key functionality, available via feature "std".
* Provide `Timestamp` and `TimeDiff` types for time operations, with extended functionality available via feature "std".
* Provide test-only functionality, in particular a seedable RNG `TestRng` which outputs its seed on test failure. Available via a new feature "testing".

### Changed
* Extend `UnbondingPurses` to take a new field `new_validator` which represents the validator to whom tokens will be re-delegated.
* Increase `DICTIONARY_ITEM_KEY_MAX_LENGTH` to 128.
* Fixed some integer casts.
* Change prefix of formatted string representation of `ContractPackageHash` from "contract-package-wasm" to "contract-package-". Parsing from the old format is still supported.

### Deprecated
* Deprecate "gens" feature (used for providing proptest helpers) in favor of new "testing" feature.



## 1.5.0

### Added
* Provide types and functionality to support improved access control inside execution engine.
* Provide `CLTyped` impl for `ContractPackage` to allow it to be passed into contracts.

### Fixed
* Limit parsing of CLTyped objects to a maximum of 50 types deep.



## 1.4.6 - 2021-12-29

### Changed
* Disable checksummed-hex encoding, but leave checksummed-hex decoding in place.



## 1.4.5 - 2021-12-06

### Added
* Add function to `auction::MintProvider` trait to support minting into an existing purse.

### Changed
* Change checksummed hex implementation to use 32 byte rather than 64 byte blake2b digests.



## [1.4.4] - 2021-11-18

### Fixed
* Revert the accidental change to the `std` feature causing a broken build when this feature is enabled.



## [1.4.3] - 2021-11-17 [YANKED]



## [1.4.2] - 2021-11-13 [YANKED]

### Added
* Add checksummed hex encoding following a scheme similar to [EIP-55](https://eips.ethereum.org/EIPS/eip-55).



## [1.4.1] - 2021-10-23

No changes.



## [1.4.0] - 2021-10-21 [YANKED]

### Added
* Add `json-schema` feature, disabled by default, to enable many types to be used to produce JSON-schema data.
* Add implicit `datasize` feature, disabled by default, to enable many types to derive the `DataSize` trait.
* Add `StoredValue` types to this crate.

### Changed
* Support building and testing using stable Rust.
* Allow longer hex string to be presented in `json` files. Current maximum is increased from 100 to 150 characters.
* Improve documentation and `Debug` impls for `ApiError`.

### Deprecated
* Feature `std` is deprecated as it is now a no-op, since there is no benefit to linking the std lib via this crate.



## [1.3.0] - 2021-07-19

### Changed
* Restrict summarization when JSON pretty-printing to contiguous long hex strings.
* Update pinned version of Rust to `nightly-2021-06-17`.

### Removed
* Remove ability to clone `SecretKey`s.



## [1.2.0] - 2021-05-27

### Changed
* Change to Apache 2.0 license.
* Return a `Result` from the constructor of `SecretKey` rather than potentially panicking.
* Improve `Key` error reporting and tests.

### Fixed
* Fix `Key` deserialization.



## [1.1.1] - 2021-04-19

No changes.



## [1.1.0] - 2021-04-13 [YANKED]

No changes.



## [1.0.1] - 2021-04-08

No changes.



## [1.0.0] - 2021-03-30

### Added
* Initial release of types for use by software compatible with Casper mainnet.



[Keep a Changelog]: https://keepachangelog.com/en/1.0.0
[unreleased]: https://github.com/casper-network/casper-node/compare/24fc4027a...dev
[1.4.3]: https://github.com/casper-network/casper-node/compare/2be27b3f5...24fc4027a
[1.4.2]: https://github.com/casper-network/casper-node/compare/v1.4.1...2be27b3f5
[1.4.1]: https://github.com/casper-network/casper-node/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/casper-network/casper-node/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/casper-network/casper-node/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/casper-network/casper-node/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.1.0]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.0.1]: https://github.com/casper-network/casper-node/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/casper-network/casper-node/releases/tag/v1.0.0
