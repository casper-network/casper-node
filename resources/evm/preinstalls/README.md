# EVM preinstall protocol assets

These are the unchanged deployed runtime bytes formerly embedded in the local
chainspec. Filenames are lowercase runtime Keccak-256 hashes without `0x`.
The chainspec lists installation address and hash pairs in `[evm].preinstalls`.

| Contract | Address | File / runtime Keccak-256 | Bytes |
| --- | --- | --- | --- |
| Multicall3 | `0xcA11bde05977b3631167028862bE2a173976CA11` | [d5c15df687b16f2ff992fc8d767b4216323184a2bbc6ee2f9c398c318e770891.bin](d5c15df687b16f2ff992fc8d767b4216323184a2bbc6ee2f9c398c318e770891.bin) | 3808 |
| Arachnid CREATE2 deployer | `0x4e59b44847b379578588920cA78FbF26c0B4956C` | [2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989.bin](2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989.bin) | 69 |
| Safe Singleton Factory | `0x914d7Fec6aaC8cd542e72Bca78B30650d45643d7` | [2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989.bin](2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989.bin) | 69 |
| ERC-2470 Singleton Factory | `0xce0042B868300000d44A59004Da54A005ffdcf9f` | [c4d5542b53a8b779595a20a8ddd60e58a6c49d3c3decc2df83ced1c69c8ca807.bin](c4d5542b53a8b779595a20a8ddd60e58a6c49d3c3decc2df83ced1c69c8ca807.bin) | 308 |
| Permit2 | `0x000000000022D473030F116dDEE9F6B43aC78BA3` | [c67d1657868aa5146eaf24fb879fb1fdec3d2d493b3683a61c9c2f4fb2851131.bin](c67d1657868aa5146eaf24fb879fb1fdec3d2d493b3683a61c9c2f4fb2851131.bin) | 9152 |
| SenderCreator v0.8 | `0x449ED7C3e6Fee6a97311d4b55475DF59C44AdD33` | [c69a1b3a000d570bc86eb096ee63a9014a17951ad616d720882ec61432b00fcf.bin](c69a1b3a000d570bc86eb096ee63a9014a17951ad616d720882ec61432b00fcf.bin) | 1217 |
| EntryPoint v0.8 | `0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108` | [44e632a24c6f2600cbd5b5b8b4c2d372359112c8b5774297f5fd0a9e64f11f86.bin](44e632a24c6f2600cbd5b5b8b4c2d372359112c8b5774297f5fd0a9e64f11f86.bin) | 21738 |

Copy this entire directory, including attribution and license notices, to
`preinstalls/` beside each protocol package's `chainspec.toml`. Startup and
upgrade discovery validate every listed runtime when EVM is enabled.
Activation reads and verifies the files again when enabled. No download or lookup
in other protocol directories is performed.

## Runtime provenance

Genesis and protocol upgrade commit upsert the configured contracts after the
EVM predeploys when EVM execution is enabled. Both genesis storage formats use
the same upsert logic. Installation preserves existing account metadata,
balances, and storage, and rejects conflicting code.

Each `.bin` file contains raw runtime bytecode, including compiler metadata.
It must not contain hexadecimal text or creation bytecode. Runtime assets are
loaded from the selected protocol package at activation; they are not embedded
in the node executable.

## Multicall3

- Address: `0xcA11bde05977b3631167028862bE2a173976CA11`.
- Upstream commit: `b667d67ecfa5361a81e8f110234ce242613b0012`.
- Source: [official signed deployment transaction](https://github.com/mds1/multicall3/blob/b667d67ecfa5361a81e8f110234ce242613b0012/README.md#new-deployments).
- Deployment transaction hash: `0x07471adfe8f4ec553c1199f495be97fc8be8e0626ae307281c22534460184ed1`.
- Runtime length: 3,808 bytes.
- Runtime Keccak-256: `0xd5c15df687b16f2ff992fc8d767b4216323184a2bbc6ee2f9c398c318e770891`.
- License: MIT; the upstream notice is retained in `LICENSE.multicall3`.

The deployment's 3,840-byte input begins with the 32-byte creation prefix
`608060405234801561001057600080fd5b50610ee0806100206000396000f3fe`.
That prefix copies and returns the remaining 3,808 bytes as runtime code.
`d5c15df687b16f2ff992fc8d767b4216323184a2bbc6ee2f9c398c318e770891.bin` contains those remaining bytes unchanged. Its address, length,
and hash are pinned by integration tests; no network access or Solidity compiler is
needed during compilation, genesis, or protocol upgrade.

## Arachnid CREATE2 deployer

- Address: `0x4e59b44847b379578588920cA78FbF26c0B4956C`.
- Upstream commit: `be3c5974db5028d502537209329ff2e730ed336c`.
- Source: [official signed deployment transaction](https://github.com/Arachnid/deterministic-deployment-proxy/blob/be3c5974db5028d502537209329ff2e730ed336c/README.md#deployment-transaction).
- Deployment transaction hash: `0xeddf9e61fb9d8f5111840daef55e5fde0041f5702856532cdbb5a02998033d26`.
- Runtime length: 69 bytes.
- Runtime Keccak-256: `0x2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989`.
- License: Unlicense; retained in `LICENSE.create2-deployer`.

The deployment's 83-byte input starts with the 14-byte creation prefix
`604580600e600039806000f350fe`. The remaining 69 bytes are returned unchanged
as runtime code and stored in `2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989.bin`. They match `eth_getCode`
on Ethereum mainnet at block 26,104,339. The factory has no constructor storage
or runtime immutables to initialize.

## Safe Singleton Factory

- Address: `0x914d7Fec6aaC8cd542e72Bca78B30650d45643d7`.
- Upstream commit: `e550557e891dc0418c46daf785198dc05df6645c`.
- Source: [official Ethereum deployment transaction](https://github.com/safe-fndn/safe-singleton-factory/blob/e550557e891dc0418c46daf785198dc05df6645c/artifacts/1/deployment.json).
- Deployment transaction hash: `0x69c275b5304db980105b7a6d731f9e1157a3fe29e7ff6ff95235297df53e9928`.
- Runtime length: 69 bytes.
- Runtime Keccak-256: `0x2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989`.
- License: MIT; the upstream notice is retained in `LICENSE.safe-singleton-factory`.

The deployment has the same 14-byte creation prefix and 69-byte runtime as
the Arachnid proxy. `2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989.bin` contains that runtime unchanged;
it matches `eth_getCode` on Ethereum mainnet at block 26,104,339. Its distinct
canonical address comes from Safe's deployer account, not a bytecode change.
No constructor storage or runtime immutables need initialization. Both
factories share a content-addressed bytecode record in global state.

## ERC-2470 Singleton Factory

- Address: `0xce0042B868300000d44A59004Da54A005ffdcf9f`.
- Specification commit: `8dd085d159cb123f545c272c0d871a5339550e79`.
- Source: [the specification's signed deployment transaction](https://github.com/ethereum/ercs/blob/8dd085d159cb123f545c272c0d871a5339550e79/ERCS/erc-2470.md#deployment-transaction).
- Deployment transaction hash: `0x803351deb6d745e91545a6a3e1c0ea3e9a6a02a1a4193b70edfcd2f40f71a01c`.
- Runtime length: 308 bytes.
- Runtime Keccak-256: `0xc4d5542b53a8b779595a20a8ddd60e58a6c49d3c3decc2df83ced1c69c8ca807`.
- License: CC0-1.0; retained in `LICENSE.erc2470-singleton-factory`.

The deployment's 340-byte input starts with the 32-byte creation prefix
`608060405234801561001057600080fd5b50610134806100206000396000f3fe`.
The remaining 308 bytes are returned unchanged as runtime code and stored in
`c4d5542b53a8b779595a20a8ddd60e58a6c49d3c3decc2df83ced1c69c8ca807.bin`. They match `eth_getCode` on Ethereum mainnet
at block 26,104,339. The factory needs no constructor storage or runtime
immutable initialization.

## Permit2

- Address: `0x000000000022D473030F116dDEE9F6B43aC78BA3`.
- Upstream commit: `cc56ad0f3439c502c246fc5cfcc3db92bb8b7219`.
- Source: Ethereum mainnet runtime, fetched with `eth_getCode` at block 26,104,339
  through `https://ethereum-rpc.publicnode.com`.
- Upstream reference: [precompiled runtime utility](https://github.com/Uniswap/permit2/blob/cc56ad0f3439c502c246fc5cfcc3db92bb8b7219/test/utils/DeployPermit2.sol).
- Runtime length: 9,152 bytes.
- Runtime Keccak-256: `0xc67d1657868aa5146eaf24fb879fb1fdec3d2d493b3683a61c9c2f4fb2851131`.
- License: MIT; the upstream notice is retained in `LICENSE.permit2`.

`c67d1657868aa5146eaf24fb879fb1fdec3d2d493b3683a61c9c2f4fb2851131.bin` contains the deployed runtime unchanged, including constructor
immutables. It matches the pinned upstream utility's runtime except for the
constructor-derived cached chain ID and domain separator. The mainnet cached
chain ID is 1 and its separator is
`0x866a5aba21966af95d6c7ab78eb2b2fc913915c28be3b9aa07cc04ff903e3f28`.
The [EIP-712 implementation](https://github.com/Uniswap/permit2/blob/cc56ad0f3439c502c246fc5cfcc3db92bb8b7219/src/EIP712.sol)
recomputes the separator when `block.chainid` differs from the cache. Integration
tests exercise cached and recomputed domains, accept a signed allowance on
chain 7, and reject replay. No constructor storage initialization is required.

## SenderCreator v0.8

- Address: `0x449ED7C3e6Fee6a97311d4b55475DF59C44AdD33`.
- Release: `eth-infinitism/account-abstraction` v0.8.0.
- Upstream commit: `4cbc06072cdc19fd60f285c5997f4f7f57a588de`.
- Source: Ethereum mainnet runtime, fetched with `eth_getCode` at block 26,104,339
  through `https://ethereum-rpc.publicnode.com`.
- Creator: EntryPoint v0.8 at `0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108`,
  using CREATE nonce 1 during construction.
- Upstream reference: [canonical EntryPoint deployment artifact](https://github.com/eth-infinitism/account-abstraction/blob/4cbc06072cdc19fd60f285c5997f4f7f57a588de/deployments/ethereum/EntryPoint.json).
- Runtime length: 1,217 bytes.
- Runtime Keccak-256: `0xc69a1b3a000d570bc86eb096ee63a9014a17951ad616d720882ec61432b00fcf`.
- License: GPL-3.0 (upstream SPDX identifier); retained in `LICENSE.account-abstraction-v08`.

`c69a1b3a000d570bc86eb096ee63a9014a17951ad616d720882ec61432b00fcf.bin` contains the deployed runtime unchanged. Its immutable
authorized EntryPoint address occurs at three call/getter sites. Replacing
that address with the compiler's zero placeholder reproduces the 1,217-byte
runtime template embedded in the pinned EntryPoint creation bytecode at
offset 22,958. The helper has no constructor storage to initialize.

The matching EntryPoint and SenderCreator runtimes must be activated together
at their canonical addresses. Direct runtime installation executes neither
constructor. The corresponding [Solidity sources and compiler settings](https://github.com/eth-infinitism/account-abstraction/blob/4cbc06072cdc19fd60f285c5997f4f7f57a588de/deployments/ethereum/solcInputs/594e0595d5f1f4861d3b32a5f38cc32f.json)
are pinned in the upstream deployment's complete compiler input.

## EntryPoint v0.8

- Address: `0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108`.
- Release: `eth-infinitism/account-abstraction` v0.8.0.
- Upstream commit: `4cbc06072cdc19fd60f285c5997f4f7f57a588de`.
- Source: Ethereum mainnet runtime, fetched with `eth_getCode` at block 26,104,339
  through `https://ethereum-rpc.publicnode.com`.
- Upstream reference: [canonical deployment artifact](https://github.com/eth-infinitism/account-abstraction/blob/4cbc06072cdc19fd60f285c5997f4f7f57a588de/deployments/ethereum/EntryPoint.json).
- Deployment transaction hash: `0xae4eafd1fd17e03b5200f6e2ecad4f669b41d9ba3a4fa0018e92cde985b33129`.
- Runtime length: 21,738 bytes.
- Runtime Keccak-256: `0x44e632a24c6f2600cbd5b5b8b4c2d372359112c8b5774297f5fd0a9e64f11f86`.
- License: GPL-3.0 (upstream SPDX identifier), retained in `LICENSE.account-abstraction-v08`;
  OpenZeppelin Contracts v5.1.0's MIT notice is retained in `LICENSE.openzeppelin-v510`.

`44e632a24c6f2600cbd5b5b8b4c2d372359112c8b5774297f5fd0a9e64f11f86.bin` contains the deployed runtime unchanged. The artifact's
`deployedBytecode` is a compiler template with zero immutable placeholders,
and must not be installed as-is. The mainnet runtime fills those placeholders
with the matching SenderCreator address and the constructor's EIP-712 values:
the EntryPoint address, cached chain ID 1 and domain separator, hashed name and
version, and short-string encodings of `ERC4337` and `1`. The template is
embedded in the pinned creation bytecode at offset 1,168; all differences
between it and the installed runtime are these constructor immutables.

The complete compiler input linked above records Solidity 0.8.28, Cancun,
via-IR compilation, and optimizer runs 1,000,000. No compiler is needed at node
build or installation time. EntryPoint v0.8 uses a transient reentrancy guard;
the Prague EVM supports its opcodes. Deposits, stakes, and nonce mappings start
at zero, and the short EIP-712 name/version need no fallback string storage.
Existing state remains intact during upserts. Integration tests verify paired
account creation at genesis and upgrades, domain selection across chain IDs,
and repeated empty bundles through the transient guard.
