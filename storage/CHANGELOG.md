# Changelog

All notable changes to this project will be documented in this file.  The format is based on [Keep a Changelog].

[comment]: <> (Added:      new features)
[comment]: <> (Changed:    changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed:    now removed features)
[comment]: <> (Fixed:      any bug fixes)
[comment]: <> (Security:   in case of vulnerabilities)



## 4.0.1

### Changed
* Changed the withdraw bid behavior to return an UnbondingAmountTooLarge error instead of forcing a unbonding of the valdiator's bid 

### Fixed
* Fixed an issue in the storage create which allowed delegators to exceed the maximum limit set by the validator for the validator's bid

## 4.0.0

### Added
* Added `maximum_delegation_amount` field to the runtime native config struct.

### Fixed
* Fixed an issue regarding incorrect setting of delegator min max limits on validator bids

## 3.0.0

### Changed
* Update `casper-types` to v4.0.1, requiring a major version bump here.



## 2.0.0

### Added
* Add `ChunkWithProof` to support chunking of large values, and associated Merkle-proofs of these.



## 1.4.4

### Changed
* Update dependencies.



## 1.4.0

### Added
* Initial release of crate providing `Digest` type and hashing methods, including the structs to handle proofs for chunks of data.



[Keep a Changelog]: https://keepachangelog.com/en/1.0.0
[unreleased]: https://github.com/casper-network/casper-node/tree/dev
