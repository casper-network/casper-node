# EVM Support Prototype

This document describes the current EVM support implemented in this
workspace, how signed Ethereum transactions move through Casper node
execution, and how to reproduce the working Foundry deployment flow.

The current scope is intentionally narrow. Casper node can accept and execute
`Transaction::Evm` transactions, store EVM execution results, and serve
read-only EVM calls through binary-port speculative execution consumed by sidecar. Native
Ethereum JSON-RPC remains a sidecar concern.

## EIP Glossary

Only EIPs referenced by this document or the current code are listed here.

| EIP | Link | Quick description |
| --- | --- | --- |
| [EIP-55][eip-55] | <https://eips.ethereum.org/EIPS/eip-55> | Mixed-case checksum encoding for Ethereum addresses. Casper has similar checksummed hex helpers. |
| [EIP-155][eip-155] | <https://eips.ethereum.org/EIPS/eip-155> | Replay protection for legacy transactions by including a chain ID in the signing payload. EVM transactions must carry the configured Casper EVM chain ID. |
| [EIP-2718][eip-2718] | <https://eips.ethereum.org/EIPS/eip-2718> | Typed transaction envelope format used by post-legacy Ethereum transaction types. The decoder accepts typed envelopes through Alloy. |
| [EIP-2930][eip-2930] | <https://eips.ethereum.org/EIPS/eip-2930> | Optional access-list transaction type. Empty access-list transactions decode; non-empty access lists are rejected for now. |
| [EIP-1559][eip-1559] | <https://eips.ethereum.org/EIPS/eip-1559> | Dynamic-fee transaction type with max fee and priority fee. Casper accepts this envelope for tooling compatibility only when the priority fee is zero. |
| [EIP-4844][eip-4844] | <https://eips.ethereum.org/EIPS/eip-4844> | Blob transaction support. Rejected because blob sidecars, blob gas, and KZG data are not modeled. |
| [EIP-7702][eip-7702] | <https://eips.ethereum.org/EIPS/eip-7702> | Set-code transactions for EOAs. Type `0x04` transactions are accepted with non-empty authorization lists; Casper still rejects non-empty access lists and non-zero priority fees. |

## Current Scope

Implemented in this workspace:

- `casper_types::evm` wrapper types for EVM addresses, hashes,
  transactions, accounts, config, and receipts.
- `Transaction::Evm` and `TransactionHash::Evm`.
- `ExecutionResult::Evm` carrying EVM receipt data.
- Global-state keys and values for EVM account identity links, nonce, code
  hash, bytecode, and storage.
- EVM hash wrappers backed by Casper `Digest` while preserving raw Ethereum
  32-byte values.
- `casper-executor-evm`, backed by `revm`, with Casper-owned public types.
- Contract runtime execution for finalized `Transaction::Evm` values.
- Casper fee and refund handling for EVM transactions.
- Binary-port `TrySpeculativeExec` for read-only `eth_call` support.
- Native Casper transfers to 20-byte EVM addresses when `[evm].enabled = true`,
  creating or funding the corresponding EVM-native purse identity.
- [EIP-7702][eip-7702] type `0x04` set-code transactions, with authorization
  lists passed through to `revm` for Prague execution.

Implemented in the sidecar workspace for validation:

- Minimal Ethereum JSON-RPC methods on the existing `/rpc` endpoint:
  `eth_chainId`, `eth_blockNumber`, `eth_getBlockByNumber`,
  `eth_getTransactionCount`, `eth_sendRawTransaction`,
  `eth_getTransactionReceipt`, and `eth_call`.
- `eth_getTransactionReceipt` projects logs stored in
  `ExecutionResult::Evm` as Ethereum receipt log entries.
- Development-only Cargo patches pointing sidecar at this node workspace for
  unreleased `casper-types` and `casper-binary-port` changes.

Not implemented yet:

- Native Ethereum JSON-RPC in node.
- `eth_estimateGas`, `eth_getBalance`, `eth_getCode`, historical `eth_call`,
  `eth_getLogs`, or `eth_getTransactionByHash`.
- Ethereum filter and subscription methods, including `eth_newFilter`,
  `eth_getFilterChanges`, `eth_getFilterLogs`, `eth_uninstallFilter`, and
  `eth_subscribe`.
- [EIP-4844][eip-4844] blob transactions.
- Non-empty [EIP-2930][eip-2930]/[EIP-1559][eip-1559] access lists.
- Non-empty [EIP-7702][eip-7702] access lists and non-zero priority fees.
- EVM log indexing optimized for historical queries.

## Transaction Shape

`Transaction::Evm` stores `casper_types::evm::Transaction` directly, like the
other top-level transaction variants. It is not boxed, and it is not stored as
a raw signed RLP blob. The EVM transaction is stored as:

- Casper envelope metadata: `timestamp` and `ttl`.
- Decoded unsigned Ethereum payload fields:
  `kind`, `chain_id`, `nonce`, gas fields, `value`, `input`, and `to`.
- [EIP-7702][eip-7702] authorization-list items when `kind` is `Eip7702`.
- Claimed/recovered EVM sender address: `from`.
- Ethereum signed transaction hash: `hash`.
- Exactly one Casper `Approval` containing the Ethereum secp256k1 signature.

`evm::Hash`, `evm::Topic`, and `evm::TransactionHash` are `Digest`-backed
wrappers, but their constructors preserve the supplied 32 bytes as raw
Ethereum values. They do not hash the bytes again. `evm::Hash` is used for
EVM bytecode hashes and other hash-shaped EVM values. `evm::Topic` is used for
EVM log topics. EVM storage slots and storage values use Casper `U256`, matching
revm's `StorageKey = U256` and `StorageValue = U256` boundary. `evm::TransactionHash`
is the Ethereum transaction hash produced from the signed Ethereum envelope.

The Ethereum transaction hash remains Ethereum-compatible. For an EVM
transaction:

```text
Transaction::hash() == TransactionHash::Evm(evm_transaction.hash())
```

The EVM hash is the Ethereum signed transaction hash, not a Casper hash of the
serialized `Transaction::Evm` wrapper.

## RLP To Approval Conversion

The raw signed Ethereum RLP transaction is handled outside node by sidecar.
The current `eth_sendRawTransaction` flow is:

1. Sidecar receives raw signed Ethereum RLP.
2. Sidecar calls:

   ```rust
   evm::Transaction::from_signed_rlp(raw, Timestamp::now(), ttl)
   ```

3. `from_signed_rlp` decodes the Ethereum envelope.
4. It rejects unsupported transaction forms:
   - [EIP-4844][eip-4844] blob transactions.
   - Non-empty access lists.
   - Unknown typed transactions.
5. For [EIP-7702][eip-7702] type `0x04`, it requires a non-empty
   authorization list and a call target, then stores authorization tuples as
   EVM transaction data.
6. It extracts the unsigned Ethereum payload fields.
7. It recovers the secp256k1 public key and EVM address.
8. It converts the Ethereum signature into one Casper `Approval`:
   - `Approval.signer` is the recovered secp256k1 public key.
   - `Approval.signature` is the 64-byte secp256k1 signature.
9. It stores the Ethereum signed transaction hash.
10. Sidecar wraps the value as `Transaction::Evm` and submits it to node over
   the existing binary-port transaction submission path.

Node does not receive the raw RLP blob for `eth_sendRawTransaction`. Node
receives a typed `Transaction::Evm` whose approval set contains the Ethereum
signature.

## Approval Handling

`Transaction::Evm` uses the same approval container and approval identity
mechanics as other transaction variants:

- `Transaction::approvals()` returns the EVM approval set.
- `Transaction::compute_approvals_hash()` computes the hash of that approval
  set.
- `Transaction::compute_id()` combines `TransactionHash::Evm` with the EVM
  approvals hash.
- Finalized approvals checksums include EVM approvals through the same storage
  mechanism used by other transactions.
- Block execution computes approvals checksums across EVM and non-EVM
  transactions.

The cryptographic verification is EVM-specific. Deploy and V1 transaction
approvals verify Casper signatures over Casper transaction hashes.
`Transaction::Evm::verify()` reconstructs the signed Ethereum envelope using
the stored payload and approval, then checks:

- There is exactly one approval.
- The approval uses secp256k1.
- One of the valid secp256k1 recovery IDs recovers the approval signer.
- The recovered EVM address matches the stored `from`.
- The reconstructed Ethereum signed transaction hash matches the stored EVM
  hash.

This avoids double signing. The Ethereum signature becomes the single Casper
approval for the `Transaction::Evm` value.

The generic `Transaction::sign` API also has an EVM implementation. For
`Transaction::Evm`, it requires a secp256k1 secret key, signs the Ethereum
signing hash using recoverable prehash semantics, replaces the approval set
with exactly one Ethereum-style approval, recomputes `from`, and recomputes the
Ethereum signed transaction hash. `evm::Transaction::try_sign` returns an
error for non-secp256k1 keys; the infallible top-level `Transaction::sign`
path panics with a clear message, matching the existing infallible signing API
style.

Approvals for EVM transactions are part of the Ethereum transaction identity.
Changing the approval changes the reconstructed signed Ethereum envelope and
therefore the Ethereum transaction hash. For that reason finalized approval
handling treats the EVM approval as immutable transaction content. The
top-level approval replacement hook used after storage lookup leaves
`Transaction::Evm` unchanged.

## Acceptor Validation

`Transaction::Evm` is converted into `MetaTransaction::Evm` and routed through
the normal transaction acceptor skeleton. EVM branches only where the payload
genuinely differs from Deploy and native Transaction::V1 payloads: identity, balance
lookup, no Casper session/payment validation, and EVM-specific chainspec
checks.

For client-submitted EVM transactions, the acceptor currently validates:

1. `[evm].enabled` must be true.
2. `evm_transaction.verify()` must pass.
3. The transaction must not be expired.
4. `evm_transaction.chain_id()` must be present.
5. The EVM chain ID must equal `[evm].chain_id`.
6. The EVM gas limit must not exceed `[evm].block_gas_limit`.
7. Legacy and [EIP-2930][eip-2930] gas price must be at least
   `[evm].base_fee`.
8. [EIP-1559][eip-1559] `max_fee_per_gas` must be at least
   `[evm].base_fee`.
9. [EIP-1559][eip-1559] `max_priority_fee_per_gas` must be zero because
   Casper does not currently prioritize transactions based on transaction gas
   parameters.
10. The EVM account identity for `from` must resolve to a balance, or the
    recovered secp256k1 signer must resolve to a Casper account balance or the
    address's deterministic EVM purse balance.
11. The transaction nonce must match the EVM nonce in global state, defaulting
    to `0` before the first EVM transaction for that address.
12. That balance must meet the chain baseline motes requirement.

The acceptor does not require a Casper `AddressableEntity` for every EVM
address. The sender identity is still
`InitiatorAddr::EvmAddress(transaction.from())`. If the EVM address is linked to
`Key::Account(account_hash)`, the Casper account's main purse is used. If it is
an EVM-native identity, the stored purse is used. If no EVM identity exists yet,
the acceptor uses the recovered signer public key to check the corresponding
Casper account balance, falling back to the address's deterministic EVM purse
when no Casper account exists. The runtime later checks the full EVM maximum
fee amount.

The nonce check is also applied to peer-sourced EVM transactions before storage,
so a gossiped transaction with a nonce that cannot execute at the current state
root is rejected before it can enter the transaction buffer.

## Runtime Execution

Finalized block execution now routes EVM transactions through the same
per-transaction accounting skeleton used by Deploy and native Transaction::V1 payloads.
Runtime constructs `MetaTransaction::Evm` before entering the loop's normal
balance, hold, refund, fee, and artifact builder flow.

At the start of each loop iteration runtime checks whether the stored
transaction is EVM:

```text
evm_transaction = stored_transaction.as_evm()
meta_transaction = MetaTransaction::from_transaction(stored_transaction, ...)
```

Common metadata such as hash, initiator, authorization keys, size estimate,
gas limit, and cost is derived directly from `Transaction`. For EVM:

- the initiator is `InitiatorAddr::EvmAddress(transaction.from())`,
- the transaction lane is currently the last configured Wasm lane,
- the gas limit is the Ethereum transaction gas limit,
- the maximum cost is `gas_limit * effective_gas_price`,
- payment is treated as standard-payment-like,
- custom payment and refund-purse setup are skipped.

Configuration compliance is enforced by the acceptor through
`MetaTransaction::is_config_compliant`. Finalized block execution expects block
contents to have passed those checks and does not duplicate the acceptor's EVM
enablement, signature, TTL, chain ID, base-fee, or priority-fee validation.

Runtime still checks the payment/accounting conditions that depend on current
state. If the EVM sender cannot cover the required balance, or if execution is
otherwise disallowed by the shared accounting loop, runtime stores an
`ExecutionResult::Evm` with a failure receipt, zero cost, zero consumed amount,
and no EVM execution effects. The EVM receipt status is typed; EVM receipts do
not persist a free-form error string for revert, halt, or accounting
precondition failure.

When execution proceeds:

1. Runtime resolves the EVM origin into a concrete payer
   (`BalanceIdentifier::Account` for linked Casper accounts or
   `BalanceIdentifier::Purse` for EVM-native identities) and creates a
   processing hold against that payer.
2. Runtime enters the shared execution `match` through the `_ if is_evm` arm.
3. Runtime checks out a tracking copy at the current scratch state root.
4. Runtime builds an EVM block context from Casper block data:
   - block height,
   - block timestamp,
   - deterministic proposer-derived beneficiary,
   - `[evm].block_gas_limit`,
   - `[evm].base_fee`.
5. Runtime calls `casper-executor-evm`.
6. `revm` executes EVM account, nonce, code, storage, log, create, and value
   transfer semantics.
7. Runtime commits EVM tracking-copy effects into scratch global state.
8. `ExecutionArtifactBuilder` records the EVM receipt, EVM effects, and the
   consumed amount.
9. Runtime clears the processing hold.
10. Runtime applies Casper refund handling.
11. Runtime applies Casper fee handling.
12. Runtime stores `ExecutionResult::Evm`.

The executor always disables `revm` gas fee balance mutation. Casper runtime
owns fee and refund policy.

## Fee And Refund Policy

EVM transactions deliberately use Casper chain-level fee and refund policy,
rather than silently emulating Ethereum's full gas escrow semantics.

Runtime computes:

```text
effective_gas_price = transaction.effective_gas_price([evm].base_fee)
max_fee_amount = gas_limit * effective_gas_price
```

The current chainspec base fee is:

```text
[evm].base_fee = 1_000_000 motes per EVM gas
```

At the current `evm.block_gas_limit` of 30,000,000 gas, filling the EVM block
gas limit costs 30,000 CSPR before any refund policy is applied:

```text
30,000,000 gas * 1,000,000 motes/gas = 30,000,000,000,000 motes
30,000,000,000,000 motes / 1,000,000,000 = 30,000 CSPR
```

For legacy and [EIP-2930][eip-2930] transactions, the effective gas price is
the signed gas price. For [EIP-1559][eip-1559] transactions, the acceptor
requires `max_priority_fee_per_gas == 0` because Casper does not currently
prioritize transactions based on transaction gas parameters. Accepted EIP-1559
transactions therefore pay `[evm].base_fee`; `max_fee_per_gas` is only a
sender cap and must be high enough to cover the base fee.

The maximum fee is held from the resolved EVM payer. After execution:

- Successful execution consumes `gas_used * effective_gas_price`.
- Failed/reverted/halted execution consumes the full held amount.
- The unconsumed portion is processed through Casper `RefundHandling`.
- The final fee is processed through Casper `FeeHandling`.

This keeps EVM transactions aligned with the same chain policy knobs used by
Deploy and native Transaction::V1 payloads. The EVM gas price is converted to motes before
calling the balance/fee/refund machinery.

In the shared accounting loop, EVM cost is already expressed as motes. Refund
calculation therefore uses `cost_to_use()` with an effective runtime gas price
of `1` for the refund-mode call, instead of multiplying by Casper's current
native transaction gas price again. This prevents double scaling while still
allowing `RefundHandling::{NoRefund,Burn,Refund}` and
`FeeHandling::{NoFee,Burn,PayToProposer,Accumulate}` to apply uniformly.

EVM does not support Casper custom payment or refund-purse selection in this
prototype. That is intentional: Ethereum payloads do not carry Casper payment
code, but the chain still owns fee and refund policy. The EVM sender's main
purse is the payer for the processing hold, refund calculation, and final fee
handling.

## Global State Layout

EVM state is stored in Casper global state using typed keys and values:

- `Key::Evm(EvmAddr::Account(Address))` stores a minimal identity pointer as
  `StoredValue::CLValue(Key::Account(AccountHash))` for linked Casper accounts
  or `StoredValue::CLValue(Key::URef(URef))` for EVM-native accounts.
- `Key::Evm(EvmAddr::Nonce(Address))` stores `StoredValue::CLValue(u64)`.
- `Key::Evm(EvmAddr::CodeHash(Address))` stores
  `StoredValue::CLValue(evm::Hash)`.
- `Key::Evm(EvmAddr::ByteCode(Hash))` stores
  `StoredValue::ByteCode(ByteCode)`.
- `Key::Evm(EvmAddr::Storage(StorageAddr))` stores
  `StoredValue::CLValue(U256)`.

`StoredValue::Evm` is not part of the current layout.

Balances are Casper purse balances. EVM balance reads and writes reconcile
through either the linked Casper account main purse or the EVM-native purse and
`Key::Balance(main_purse.addr())`.

Genesis does not create EVM account records for Casper genesis accounts.
Funding an EVM identity is explicit: a native Casper transfer can use a
20-byte `evm::Address` as its `target` argument when `[evm].enabled = true`.
If `Key::Evm(EvmAddr::Account(address))` already exists, the transfer credits
the linked account or purse. If it does not exist, the transfer writes an
EVM-native identity pointing to `evm::deterministic_purse(address)`, initializes
`Nonce(address)` to `0`, initializes `CodeHash(address)` to
`EMPTY_CODE_HASH`, initializes that deterministic purse with a zero balance,
then transfers the requested motes into it. Transfer records keep the Casper
transfer schema unchanged: `to` is `None`, and `target` is the EVM account's
backing purse.

When a signed EVM transaction is executed for an address without a linked Casper
identity, contract runtime uses the transaction approval to recover the
secp256k1 public key before invoking the EVM executor. If the corresponding
`Key::Account(account_hash)` already exists and no established EVM-native
identity conflicts with it, `EvmAddr::Account(address)` is written as a bridge
to that Casper account. If no Casper account exists, contract runtime creates a
Casper account for that account hash backed by
`evm::deterministic_purse(address)`, then writes the bridge. EVM addresses
created by contract creation or normal runtime effects are not forced to have an
account hash; they remain EVM-native purse identities.

## Receipts

Runtime stores EVM receipts in `ExecutionResult::Evm`.

The receipt currently contains:

- typed status: `Success`, `Revert`, or `Halt(reason)`,
- gas used,
- effective gas price,
- created contract address,
- logs.

Sidecar maps the typed receipt status to Ethereum JSON-RPC receipt status:
`Success` becomes `0x1`; `Revert` and `Halt(_)` become `0x0`. The typed
Casper receipt keeps the extra distinction between revert and exceptional
halt, but Ethereum receipt projection remains compatible with standard tools.

`ExecutionResult::Evm` stores the Casper accounting fields needed by existing
execution APIs: initiator, limit, cost, refund, current price, size estimate,
effects, and receipt. It does not store an `error_message`; EVM failure detail
lives in the typed receipt status.

Block-derived Ethereum JSON-RPC fields are not persisted in the receipt.
Sidecar derives those fields from execution info and block transaction order:

- block hash,
- block number,
- transaction index,
- cumulative gas used,
- log indexes,
- logs bloom,
- removed flag.

## Read-only EVM Calls

`eth_call` uses the node binary-port `TrySpeculativeExec` command, not
`TryAcceptTransaction` submission.

Sidecar constructs a `Transaction::Evm` with
`evm::Transaction::new_unsigned_call`, which carries:

- chain ID,
- `from`,
- `to`,
- `value`,
- input bytes,
- gas limit,
- gas price.

Node handles the request only when speculative execution is enabled for the binary port.
The unsigned call still passes EVM config compliance checks, including EVM
enablement, chain ID, gas price, and block gas limit. It only
skips signature verification because read-only `eth_call` requests are not
signed Ethereum transactions. The transaction acceptor still rejects this
marker shape so unsigned calls cannot be submitted through `TryAcceptTransaction`.
Contract runtime checks out state at the requested/latest block,
runs `casper-executor-evm` with:

- `ExecuteKind::Call`,
- `CallValidation::UncheckedSimulation`,

and returns output, receipt status, and gas used in
`EvmSpeculativeExecutionResult`, carried by the contract-runtime
`SpeculativeExecutionResult::Evm` variant.
The tracking-copy effects are discarded.

## Block Hashes

The executor exposes `BLOCK_HASH_HISTORY`, mirroring revm's Ethereum
`BLOCKHASH` history window. Node runtime and binary-port EVM calls use that
public executor constant when loading recent block hashes, so `casper-node`
does not need a production dependency on `revm`.

Block hashes are loaded as Casper `BlockHash` values and converted to revm's
hash type only at the executor API boundary. If a contract requests a future
block, the current block, or a block outside the supported history window,
`BLOCKHASH` returns zero.

## Chain ID

The current local devnet EVM chain ID is:

```text
0x435350ff
```

That is the Casper namespace prefix plus the local-network namespace. Sidecar
reports it through `eth_chainId`, and node requires signed EVM transactions to
carry the same value.

## Reproducing With Foundry

The following flow deploys and calls the EVM `Counter` fixture through
`casper-sidecar`.

### Prerequisites

Install the usual node build dependencies plus Foundry:

```bash
forge --version
cast --version
```

Set shell variables for the local checkouts used below:

```bash
export CASPER_NODE_WORKSPACE=/path/to/casper-node
export CASPER_SIDECAR_WORKSPACE=/path/to/casper-sidecar
```

The sidecar workspace must be patched to the node workspace because these EVM
types are unreleased. In `$CASPER_SIDECAR_WORKSPACE/Cargo.toml`:

```toml
[patch."https://github.com/casper-network/casper-node.git"]
casper-binary-port = { path = "/path/to/casper-node/binary_port" }
casper-types = { path = "/path/to/casper-node/types" }
```

If the node workspace path changes, update the patch paths.

### Build Node And Sidecar

From the node workspace:

```bash
cargo build -p casper-node --bin casper-node
```

From the sidecar workspace:

```bash
cd "$CASPER_SIDECAR_WORKSPACE"
cargo build -p casper-sidecar
```

### Install And Configure Devnet

`casper-devnet` is a separate development tool. It is not part of this
`casper-node` repository, so `cargo run -- ...` only works from a
`casper-devnet` checkout, not from this workspace.

The devnet tool needs a custom asset named `evm` that points at the debug node
and sidecar binaries built above, plus the local chainspec and config files
from this workspace. Use a node config where
`[binary_port_server].allow_request_speculative_exec = true`; the checked-in local
config defaults this to `false`, so copy `resources/local/config.toml` and
enable it in the copy used for this custom asset.

For example:

```bash
export EVM_DEVNET_NODE_CONFIG=/tmp/casper-node-evm-devnet-config.toml
cp "$CASPER_NODE_WORKSPACE/resources/local/config.toml" "$EVM_DEVNET_NODE_CONFIG"
# Edit $EVM_DEVNET_NODE_CONFIG so allow_request_speculative_exec = true.
```

From a separate `casper-devnet` checkout, register the asset with:

```bash
cd /path/to/casper-devnet
cargo run -- assets add evm \
    --casper-node "$CASPER_NODE_WORKSPACE/target/debug/casper-node" \
    --casper-sidecar "$CASPER_SIDECAR_WORKSPACE/target/debug/casper-sidecar" \
    --chainspec "$CASPER_NODE_WORKSPACE/resources/local/chainspec.toml" \
    --node-config "$EVM_DEVNET_NODE_CONFIG" \
    --sidecar-config "$CASPER_SIDECAR_WORKSPACE/resources/example_configs/default_rpc_only_config.toml"
```

If `casper-devnet` is already installed on `PATH`, the equivalent command is:

```bash
casper-devnet assets add evm \
    --casper-node "$CASPER_NODE_WORKSPACE/target/debug/casper-node" \
    --casper-sidecar "$CASPER_SIDECAR_WORKSPACE/target/debug/casper-sidecar" \
    --chainspec "$CASPER_NODE_WORKSPACE/resources/local/chainspec.toml" \
    --node-config "$EVM_DEVNET_NODE_CONFIG" \
    --sidecar-config "$CASPER_SIDECAR_WORKSPACE/resources/example_configs/default_rpc_only_config.toml"
```

Register the asset once for a given set of paths. Rebuilding the node or
sidecar binaries does not require re-adding the asset as long as the asset
points at those debug binary paths. You can inspect the installed custom asset
with:

```bash
casper-devnet assets path evm
```

If the asset already exists and needs different paths, remove or recreate the
existing custom asset directory first, then run `assets add` again.

### Start Devnet

After the `evm` asset is registered, start the network from any directory where
the `casper-devnet` binary is available:

```bash
casper-devnet start --custom-asset evm --force-setup \
    --chainspec-override evm.enabled=true
```

The `evm` custom asset uses the debug node binary from
`$CASPER_NODE_WORKSPACE` and the debug sidecar binary from
`$CASPER_SIDECAR_WORKSPACE`.

Devnet is not yet fully aware of the new EVM variants, so it can log SSE decode
warnings after EVM transactions are accepted or included in blocks. Those
warnings do not block this JSON-RPC validation flow.

### Devnet User

Use the deterministic devnet `user-1` secp256k1 key:

```text
private key: 0xb6cc5d5faa7c3c37db4bf9a1566023aaa9a1d716fe78ed1a6fb79a690b9400e8
EVM address: 0x24790C4849cCAE43c0c1749e2C5b8d00Cc63AB80
```

Confirm sidecar and account state:

```bash
curl -s -X POST http://127.0.0.1:11101/rpc \
    -H 'content-type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}'

curl -s -X POST http://127.0.0.1:11101/rpc \
    -H 'content-type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"eth_getTransactionCount","params":["0x24790C4849cCAE43c0c1749e2C5b8d00Cc63AB80","latest"]}'

casper-cli account balance devnet:user-1
```

Expected initial values on a fresh devnet:

```text
eth_chainId: 0x435350ff
eth_getTransactionCount: 0x0
```

### No EVM Prefund Required

Do not fund the 20-byte EVM address before deploying. The first EVM transaction
from `user-1` recovers the secp256k1 public key, resolves the existing Casper
account, and writes the `EvmAddr::Account` identity link during execution. A
native transfer to a missing 20-byte target is still supported, but that path
creates an EVM-native purse identity instead of demonstrating Casper account
linking.

### Deploy Counter

From the node workspace:

```bash
forge create --broadcast \
    --rpc-url http://127.0.0.1:11101/rpc \
    --private-key 0xb6cc5d5faa7c3c37db4bf9a1566023aaa9a1d716fe78ed1a6fb79a690b9400e8 \
    --legacy \
    --gas-price 1000000 \
    --gas-limit 3000000 \
    --nonce 0 \
    smart_contracts/evm_contracts/Counter.sol:Counter
```

The current validation uses `--legacy` because the minimum RPC surface does
not yet include gas estimation or dynamic-fee helper methods, and Casper only
accepts [EIP-1559][eip-1559] transactions when
`max_priority_fee_per_gas == 0`. Passing an explicit legacy gas price equal to
`[evm].base_fee` keeps the transaction shape simple and avoids underpriced
transaction rejection.

Expected output:

```text
Deployer: 0x24790C4849cCAE43c0c1749e2C5b8d00Cc63AB80
Deployed to: 0x6c0704679CA22b83778Ef815607359cf6F5352B6
Transaction hash: <deployment transaction hash>
```

Forge may create local `cache/` and `out/` directories. They are build
artifacts and should not be committed.

The corresponding receipt should contain:

```text
status             0x1
contractAddress    0x6c0704679ca22b83778ef815607359cf6f5352b6
gasUsed            <non-zero gas used>
effectiveGasPrice  0xf4240
```

### Read Counter

```bash
cast call 0x6c0704679CA22b83778Ef815607359cf6F5352B6 \
    'get()(uint256)' \
    --rpc-url http://127.0.0.1:11101/rpc
```

Expected output:

```text
0
```

### Increment Counter

```bash
export COUNTER_ADDRESS=0x6c0704679CA22b83778Ef815607359cf6F5352B6

cast send "$COUNTER_ADDRESS" \
    'increment()' \
    --rpc-url http://127.0.0.1:11101/rpc \
    --private-key 0xb6cc5d5faa7c3c37db4bf9a1566023aaa9a1d716fe78ed1a6fb79a690b9400e8 \
    --legacy \
    --gas-price 1000000 \
    --gas-limit 100000 \
    --nonce 1
```

Expected receipt highlights:

```text
status               1 (success)
type                 0
effectiveGasPrice    1000000
gasUsed              <non-zero gas used, including the event LOG cost>
to                   0x6c0704679CA22b83778Ef815607359cf6F5352B6
transactionHash      0x042ff975ec4b8fa8012f486bb7bd930e69978782b8b3c107ca2a276a43d7f293
```

`increment()` emits:

```solidity
event CounterIncremented(address indexed caller, uint256 newValue);
```

For the deterministic devnet key, the increment receipt should include one log
emitted by `$COUNTER_ADDRESS`. Sidecar currently exposes emitted events through
`eth_getTransactionReceipt`, so receipt-oriented tooling can see and verify the
event:

```bash
export INCREMENT_TX_HASH=0x042ff975ec4b8fa8012f486bb7bd930e69978782b8b3c107ca2a276a43d7f293

cast receipt "$INCREMENT_TX_HASH" \
    --rpc-url http://127.0.0.1:11101/rpc \
    --json | jq '.logs[0]'

cast sig-event 'CounterIncremented(address,uint256)'
```

Expected event checks:

```text
log.address == $COUNTER_ADDRESS
log.topics[0] == 0x59950fb23669ee30425f6d79758e75fae698a6c88b2982f2980638d8bcd9397d
log.topics[1] == 0x00000000000000000000000024790c4849ccae43c0c1749e2c5b8d00cc63ab80
log.data      == 0x0000000000000000000000000000000000000000000000000000000000000001
```

That is enough for tools that validate a known transaction receipt. Generic
Ethereum event discovery, for example `cast logs`, ethers.js filters, or
web3.js filter polling, also needs sidecar support for `eth_getLogs` and the
filter/subscription RPCs listed in the current caveats.

### Read Counter Again

```bash
cast call 0x6c0704679CA22b83778Ef815607359cf6F5352B6 \
    'get()(uint256)' \
    --rpc-url http://127.0.0.1:11101/rpc
```

Expected output:

```text
1
```

### Verify EIP-7702 Set-Code

The deployed `Counter` contract can also be used as the delegate target for an
[EIP-7702][eip-7702] set-code transaction. This verifies the full path through
off-the-shelf Ethereum tooling:

- `cast wallet sign-auth` signs the authorization tuple.
- `cast send --auth` submits a type `0x04` transaction through
  `eth_sendRawTransaction`.
- `cast receipt` sees the projected receipt as `type: 0x4`.
- A later transaction without `--auth` still executes the delegated code,
  proving the delegation persisted in EVM state.

Use `user-1` as the fee payer and a separate authority EOA as the account
whose code is delegated. The authority key does not need to be funded; it only
signs the EIP-7702 authorization. The `user-1` account pays for the transaction.

```bash
export RPC_URL=http://127.0.0.1:11101/rpc
export USER_PRIVATE_KEY=0xb6cc5d5faa7c3c37db4bf9a1566023aaa9a1d716fe78ed1a6fb79a690b9400e8
export USER_ADDRESS=0x24790C4849cCAE43c0c1749e2C5b8d00Cc63AB80

export AUTHORITY_PRIVATE_KEY=0x59c6995e998f97a5a0044966f0945381cf28caaa54a7dc353d821cabcd789def
export AUTHORITY_ADDRESS=$(cast wallet address --private-key "$AUTHORITY_PRIVATE_KEY")
export COUNTER_ADDRESS=0x6c0704679CA22b83778Ef815607359cf6F5352B6

export USER_NONCE=$(cast nonce "$USER_ADDRESS" --rpc-url "$RPC_URL")
```

On a fresh devnet where the deploy and increment examples above were run,
`USER_NONCE` should be `2`. If only the deployment was run, it should be `1`.
The authority nonce should be `0` before its first authorization:

```bash
cast nonce "$AUTHORITY_ADDRESS" --rpc-url "$RPC_URL"
```

Expected output:

```text
0
```

Sign the authorization for the authority account to delegate to the deployed
`Counter` code. The chain ID is the decimal form of `0x435350ff`.

```bash
export SET_CODE_AUTH=$(cast wallet sign-auth "$COUNTER_ADDRESS" \
    --private-key "$AUTHORITY_PRIVATE_KEY" \
    --chain 1129533695 \
    --nonce 0)
```

Submit the set-code transaction. Do not pass `--legacy`; the authorization list
causes Foundry to build an EIP-7702 transaction. Pass
`--priority-gas-price 0` because Casper currently rejects non-zero priority
fees.

```bash
cast send "$AUTHORITY_ADDRESS" \
    'increment()' \
    --rpc-url "$RPC_URL" \
    --private-key "$USER_PRIVATE_KEY" \
    --auth "$SET_CODE_AUTH" \
    --gas-price 1000000 \
    --priority-gas-price 0 \
    --gas-limit 300000 \
    --nonce "$USER_NONCE" \
    --json | tee /tmp/casper-eip7702-set-code.json
```

Expected receipt checks:

```bash
export SET_CODE_TX_HASH=$(jq -r .transactionHash /tmp/casper-eip7702-set-code.json)

cast receipt "$SET_CODE_TX_HASH" \
    --rpc-url "$RPC_URL" \
    --json | tee /tmp/casper-eip7702-set-code-receipt.json

jq '{type,status,gasUsed,effectiveGasPrice,from,to,logs: [.logs[] | {address,topics,data}]}' \
    /tmp/casper-eip7702-set-code-receipt.json
```

Expected highlights:

```text
type               0x4
status             0x1
effectiveGasPrice  0xf4240
from               0x24790c4849ccae43c0c1749e2c5b8d00cc63ab80
to                 $AUTHORITY_ADDRESS
logs[0].address    $AUTHORITY_ADDRESS
logs[0].topics[0]  0x59950fb23669ee30425f6d79758e75fae698a6c88b2982f2980638d8bcd9397d
logs[0].topics[1]  0x00000000000000000000000024790c4849ccae43c0c1749e2c5b8d00cc63ab80
logs[0].data       0x0000000000000000000000000000000000000000000000000000000000000001
```

Reading `get()` through the authority address should now execute delegated
`Counter` code and return `1`. The deployed `Counter` contract has separate
storage; the authority's counter starts from zero even if the original
`Counter` was incremented earlier.

```bash
cast call "$AUTHORITY_ADDRESS" \
    'get()(uint256)' \
    --rpc-url "$RPC_URL"

cast nonce "$AUTHORITY_ADDRESS" --rpc-url "$RPC_URL"
```

Expected output:

```text
1
1
```

Finally, prove that the delegation persists after the set-code transaction by
calling the authority again with a normal legacy transaction and no
authorization list:

```bash
export USER_NONCE=$((USER_NONCE + 1))

cast send "$AUTHORITY_ADDRESS" \
    'increment()' \
    --rpc-url "$RPC_URL" \
    --private-key "$USER_PRIVATE_KEY" \
    --legacy \
    --gas-price 1000000 \
    --gas-limit 100000 \
    --nonce "$USER_NONCE" \
    --json | tee /tmp/casper-eip7702-persisted-delegation.json

cast call "$AUTHORITY_ADDRESS" \
    'get()(uint256)' \
    --rpc-url "$RPC_URL"
```

Expected output from the final `cast call`:

```text
2
```

### Confirm Fees

The native transfer debits devnet `user-1` and credits the EVM identity's
deterministic backing purse. EVM transaction fees are charged from that EVM
backing purse, not from `user-1`'s Casper account purse:

```bash
casper-cli account balance devnet:user-1
```

With `--gas-price 1000000`, every 1,000 gas consumed is 1 CSPR before refund
policy is applied.

## Useful Checks

Node workspace:

```bash
cargo check -p casper-node --bin casper-node
cargo test -p casper-binary-port --lib
```

Sidecar workspace:

```bash
cd "$CASPER_SIDECAR_WORKSPACE"
cargo test -p casper-rpc-sidecar eth --lib
cargo build -p casper-sidecar
```

## Current Caveats

- `casper-devnet` SSE parsing is not yet updated for new EVM variants.
- `eth_getBlockByNumber` currently returns enough typed fields for Foundry
  polling, but it is not a complete Ethereum block projection.
- `eth_getBlockByNumber` currently returns placeholder block-level
  `logsBloom`, `receiptsRoot`, and `gasUsed` values. Receipt-level log data is
  available through `eth_getTransactionReceipt`, but block-level receipt-root
  verification is not implemented.
- Sidecar derives receipt fields from stored `ExecutionResult::Evm` and block
  metadata; efficient historical log queries are not implemented.
- EVM call support is latest/pending only in sidecar.
- EVM support is currently a prototype path and still uses local sidecar
  patches for unreleased node types.

[eip-55]: https://eips.ethereum.org/EIPS/eip-55
[eip-155]: https://eips.ethereum.org/EIPS/eip-155
[eip-2718]: https://eips.ethereum.org/EIPS/eip-2718
[eip-2930]: https://eips.ethereum.org/EIPS/eip-2930
[eip-1559]: https://eips.ethereum.org/EIPS/eip-1559
[eip-4844]: https://eips.ethereum.org/EIPS/eip-4844
[eip-7702]: https://eips.ethereum.org/EIPS/eip-7702
