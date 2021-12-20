# Changelog

All notable changes to this project will be documented in this file.  The format is based on [Keep a Changelog].

[comment]: <> (Added:      new features)
[comment]: <> (Changed:    changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed:    now removed features)
[comment]: <> (Fixed:      any bug fixes)
[comment]: <> (Security:   in case of vulnerabilities)



## [Unreleased]

### Changed
* Increase `DICTIONARY_ITEM_KEY_MAX_LENGTH` to 128.
* Change checksummed hex implementation to use 32 byte rather than 64 byte blake2b digests. 



## [1.4.0] - 2021-10-04

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
[unreleased]: https://github.com/casper-network/casper-node/compare/v1.4.0...dev
[1.4.0]: https://github.com/casper-network/casper-node/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/casper-network/casper-node/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/casper-network/casper-node/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.1.0]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.0.1]: https://github.com/casper-network/casper-node/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/casper-network/casper-node/releases/tag/v1.0.0
