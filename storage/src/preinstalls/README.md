# EVM preinstalls

The registry in `../preinstalls.rs` lists utility contracts and their canonical
addresses.

Each `.bin` file contains raw runtime bytecode, including compiler metadata, and
is embedded with `include_bytes!`. It must not contain hexadecimal text or
creation bytecode.

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
needed during compilation.
