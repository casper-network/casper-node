# Changelog

All notable changes to this project will be documented in this file.  The format is based on [Keep a Changelog].

[comment]: <> (Added:      new features)
[comment]: <> (Changed:    changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed:    now removed features)
[comment]: <> (Fixed:      any bug fixes)
[comment]: <> (Security:   in case of vulnerabilities)

## 5.0.0

### Added

* Added system-level messaging: when a new contract version is installed or upgraded, four
  system messages are emitted on behalf of the system account advertising the package key,
  entity key, bytecode key, and version of the new deployment.
  **Note on system contracts:** The core system contracts (Mint, Auction, HandlePayment,
  StandardPayment) do not emit these system messages. They are not deployed via the standard
  WASM install/upgrade path and do not live in WASM space, so tracking them via the messaging
  system is intentionally out of scope.

* Added a field `rewards` handling to Config in the `runtime_native` module_

### Changed

* Modified the behavior of the protocol upgrade logic to add a sustain purse to the mints named keys if the rewards
  handling to sustain
* Modified the behavior of the protocol upgrade logic to recalculate the total supply at the point of protocol upgrade
* Modified the Genesis flow to support the rewards handling mode sustain in the Account/Contract model
* Modified the auction logic to keep track of a minimum delegation rate for validators

### Fixed

* Fixed a bug introduced during protocol version 2.0 in the genesis logic that did not include delegator stakes towards
  the total supply

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
