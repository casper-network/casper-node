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
* Support fetching the calling account's authorization keys via the new function `runtime::list_authorization_keys` which calls the new `ext_ffi::casper_load_authorization_keys`.
* Support providing 32 random bytes via the new function `runtime::random_bytes` which calls the new `ext_ffi::casper_random_bytes`.
* Add `storage::read_from_key` for reading a value under a given `Key`.
* Add `storage::dictionary_read` for reading a value from a dictionary under a given `Key`, calling the new `ext_ffi::casper_dictionary_read`.
* Add `storage::named_dictionary_put` for writing a named value to a named dictionary.
* Add `storage::named_dictionary_get` for reading a named value from a named dictionary.

### Changed
* Increase `DICTIONARY_ITEM_KEY_MAX_LENGTH` to 128.
* Update pinned version of Rust to `nightly-2022-08-03`.




## 1.4.4

### Changed
* Minor refactor of `system::create_purse()`.



## [1.4.0] - 2021-10-04

### Added
* Add `no-std-helpers` feature, enabled by default, which provides no-std panic/oom handlers and a global allocator as a convenience.
* Add new APIs for transferring tokens to the main purse associated with a public key: `transfer_to_public_key` and `transfer_from_purse_to_public_key`.

### Deprecated
* Feature `std` is deprecated as it is now a no-op, since there is no benefit to linking the std lib via this crate.



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
* Initial release of smart contract API compatible with Casper mainnet.



[Keep a Changelog]: https://keepachangelog.com/en/1.0.0
[unreleased]: https://github.com/casper-network/casper-node/compare/v1.4.0...dev
[1.4.0]: https://github.com/casper-network/casper-node/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/casper-network/casper-node/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/casper-network/casper-node/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.1.0]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.0.1]: https://github.com/casper-network/casper-node/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/casper-network/casper-node/releases/tag/v1.0.0
