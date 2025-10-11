# Changelog

All notable changes to this project will be documented in this file. The format is based on [Keep a Changelog].

[comment]: <> (Added: new features)
[comment]: <> (Changed: changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed: now removed features)
[comment]: <> (Fixed: any bug fixes)
[comment]: <> (Security: in case of vulnerabilities)

## [Unreleased]

### Added

- A VM2 contract version install (both new and upgrade) will now produce a native message to system-owned topics:
  - in case `addressable_entity` is turned off:
    - `contract_key` topic will receive string-formatted `Key::Hash` containing the address of the new `StoredValue::Contract` value
    - `package_key` topic will receive string-formatted `Key::Hash` containing the address of the new `StoredValue::ContractPackage` value
    - `bytecode_key` topic will receive string-formatted `Key::Hash` containing the address of the new `StoredValue::ContractWasm` value
    - `contract_version` topic will receive a string containing the major contract and minor installed contract version (for example "2.1")
  - in case `addressable_entity` is turned on:
    - `contract_key` topic will receive string-formatted `Key::AddressableEntity` containing the address of the new `AddressableEntity` of kind `SmartContract` value
    - `package_key` topic will receive string-formatted `Key::SmartContract` containing the address of the new `SmartContract::Package` value
    - `bytecode_key` topic will receive string-formatted `Key::ByteCode` containing the address of the new `StoredValue::ByteCode` value
    - `contract_version` topic will receive a string containing the major contract and minor installed contract version (for example "2.1")
