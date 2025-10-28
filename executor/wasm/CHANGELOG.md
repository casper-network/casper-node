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

- Calling wasm adds additional execution journal entries. For each session execution there will be a corresponding pair of TransformKindV2::EntryPointCalled and TransformKindV2::Ret written:
  - For session bytes:
    - the key under which both transforms will be stored is the Key::Account that initiated the transaction.
    - The `Option<HashAddr>` of `EntryPointCalled` will be `None`
    - The `String` of `EntryPointCalled` will be `"call"` (the default function name we expect of session wasm)
    - The `Ret` will either be `RetValue::Unit` or `RetValue::Bytes` depending on wether or not the code called `casper_return`.
  - If a session call calls a stored contracts entrypoint:
    - the key under which both transforms will be stored is the Key::Account that initiated the transaction.
    - The `Option<HashAddr>` of `EntryPointCalled` will be `Some(<called_contract_hash>)`
    - The `String` of `EntryPointCalled` will be the name of the called entrypoint
    - The `Ret` will either be `RetValue::Unit` or `RetValue::Bytes` depending on wether or not the code called `casper_return`.
  - If a stored contract calls another stored contract:
    - the key under which both transforms will be stored is the key of the contract which is executing the call.
    - The `Option<HashAddr>` of `EntryPointCalled` will be `Some(<called_contract_hash>)`
    - The `String` of `EntryPointCalled` will be the name of the called entrypoint
    - The `Ret` will either be `RetValue::Unit` or `RetValue::Bytes` depending on wether or not the code called `casper_return`.
