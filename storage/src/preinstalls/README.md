# EVM preinstalls

The registry in `../preinstalls.rs` contains utility contracts installed at their
canonical addresses. Genesis and protocol upgrade commit upsert these after the
EVM predeploys when EVM execution is enabled. Both genesis storage formats use
the same registry and upsert logic.

Each `.bin` file contains raw runtime bytecode, including compiler metadata, and
is embedded with `include_bytes!`. It must not contain hexadecimal text or
creation bytecode. Installation preserves existing account metadata, balances,
and storage, and rejects conflicting code.

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
`multicall3.bin` contains those remaining bytes unchanged. Its address, length,
and hash are pinned by a unit test; no network access or Solidity compiler is
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
as runtime code and stored in `create2-deployer.bin`. They match `eth_getCode`
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
the Arachnid proxy. `safe-singleton-factory.bin` contains that runtime unchanged;
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
`erc2470-singleton-factory.bin`. They match `eth_getCode` on Ethereum mainnet
at block 26,104,339. The factory needs no constructor storage or runtime
immutable initialization.
