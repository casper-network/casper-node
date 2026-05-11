# EVM Support Prototype

This document describes the current EVM support implemented in this
workspace, how signed Ethereum transactions move through Casper node
execution, and how to reproduce the working Foundry deployment flow.

The current scope is intentionally narrow. Casper node can accept and execute
`Transaction::Evm` transactions, store EVM execution results, and serve
read-only EVM calls through a binary-port command consumed by sidecar. Native
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
| [EIP-7702][eip-7702] | <https://eips.ethereum.org/EIPS/eip-7702> | Set-code transactions for EOAs. Rejected because authorization-list processing and account-code mutation are not implemented. |

## Current Scope

Implemented in this workspace:

- `casper_types::evm` wrapper types for EVM addresses, hashes,
  transactions, accounts, config, and receipts.
- `Transaction::Evm` and `TransactionHash::Evm`.
- `ExecutionResult::Evm` carrying EVM receipt data.
- Global-state keys and values for EVM account metadata, bytecode, and
  storage.
- EVM hash wrappers backed by Casper `Digest` while preserving raw Ethereum
  32-byte values.
- `casper-executor-evm`, backed by `revm`, with Casper-owned public types.
- Contract runtime execution for finalized `Transaction::Evm` values.
- Casper fee and refund handling for EVM transactions.
- Binary-port `EvmCall` for read-only `eth_call` support.
- Native Casper transfers to 20-byte EVM addresses when `[evm].enabled = true`,
  creating or funding the corresponding EVM account record.

Implemented in the sidecar workspace for validation:

- Minimal Ethereum JSON-RPC methods on the existing `/rpc` endpoint:
  `eth_chainId`, `eth_blockNumber`, `eth_getBlockByNumber`,
  `eth_getTransactionCount`, `eth_sendRawTransaction`,
  `eth_getTransactionReceipt`, and `eth_call`.
- Development-only Cargo patches pointing sidecar at this node workspace for
  unreleased `casper-types` and `casper-binary-port` changes.

Not implemented yet:

- Native Ethereum JSON-RPC in node.
- `eth_estimateGas`, `eth_getBalance`, `eth_getCode`, historical `eth_call`,
  `eth_getLogs`, or `eth_getTransactionByHash`.
- [EIP-4844][eip-4844] blob transactions.
- [EIP-7702][eip-7702] set-code transactions.
- Non-empty [EIP-2930][eip-2930]/[EIP-1559][eip-1559] access lists.
- EVM log indexing optimized for historical queries.

## Transaction Shape

`Transaction::Evm` stores `casper_types::evm::Transaction` directly, like the
other top-level transaction variants. It is not boxed, and it is not stored as
a raw signed RLP blob. The EVM transaction is stored as:

- Casper envelope metadata: `timestamp` and `ttl`.
- Decoded unsigned Ethereum payload fields:
  `kind`, `chain_id`, `nonce`, gas fields, `value`, `input`, and `to`.
- Claimed/recovered EVM sender address: `from`.
- Ethereum signed transaction hash: `hash`.
- Exactly one Casper `Approval` containing the Ethereum secp256k1 signature.

`evm::Hash` and `evm::TransactionHash` are `Digest`-backed wrappers, but their
constructors preserve the supplied 32 bytes as raw Ethereum values. They do
not hash the bytes again. `evm::Hash` is used for EVM words, storage keys,
storage values, topics, and bytecode hashes. `evm::TransactionHash` is the
Ethereum transaction hash produced from the signed Ethereum envelope.

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
   - [EIP-7702][eip-7702] set-code transactions.
   - Non-empty access lists.
   - Unknown typed transactions.
5. It extracts the unsigned Ethereum payload fields.
6. It recovers the secp256k1 public key and EVM address.
7. It converts the Ethereum signature into one Casper `Approval`:
   - `Approval.signer` is the recovered secp256k1 public key.
   - `Approval.signature` is the 64-byte secp256k1 signature.
8. It stores the Ethereum signed transaction hash.
9. Sidecar wraps the value as `Transaction::Evm` and submits it to node over
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
genuinely differs from Deploy and V1/V2 transactions: identity, balance
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
10. `BalanceIdentifier::Evm(from)` must resolve to a balance.
11. That balance must meet the chain baseline motes requirement.

The acceptor does not require a Casper `AddressableEntity` for the EVM sender.
The sender identity is `InitiatorAddr::EvmAddress(transaction.from())`, and
balance checks use `BalanceIdentifier::Evm(address)`. The acceptor only checks
that the EVM initiator has a known balance and meets the same baseline balance
requirement used for other client transactions. The runtime later checks the
full EVM maximum fee amount.

## Runtime Execution

Finalized block execution now routes EVM transactions through the same
per-transaction accounting skeleton used by Deploy and V1/V2 transactions.
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

1. Runtime creates a processing hold against
   `BalanceIdentifier::Evm(transaction.from())`.
2. Runtime enters the shared execution `match` through the `_ if is_evm` arm.
3. Runtime checks out a tracking copy at the current scratch state root.
4. Runtime builds an EVM block context from Casper block data:
   - block height,
   - block timestamp,
   - deterministic proposer-derived beneficiary,
   - `[evm].block_gas_limit`,
   - `[evm].base_fee`.
5. Runtime calls `casper-executor-evm` with `FeeCharge::External`.
6. `revm` executes EVM account, nonce, code, storage, log, create, and value
   transfer semantics.
7. Runtime commits EVM tracking-copy effects into scratch global state.
8. `ExecutionArtifactBuilder` records the EVM receipt, EVM effects, and the
   consumed amount.
9. Runtime clears the processing hold.
10. Runtime applies Casper refund handling.
11. Runtime applies Casper fee handling.
12. Runtime stores `ExecutionResult::Evm`.

`FeeCharge::External` is important. It prevents `revm` from charging gas fees
from EVM balances. Casper runtime owns fee and refund policy.

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

The maximum fee is held from `BalanceIdentifier::Evm(from)`. After execution:

- Successful execution consumes `gas_used * effective_gas_price`.
- Failed/reverted/halted execution consumes the full held amount.
- The unconsumed portion is processed through Casper `RefundHandling`.
- The final fee is processed through Casper `FeeHandling`.

This keeps EVM transactions aligned with the same chain policy knobs used by
Deploy and V1/V2 transactions. The EVM gas price is converted to motes before
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

- `Key::EvmAccount(Address)` stores `StoredValue::EvmAccount(Account)`.
- `Key::EvmByteCode(Hash)` stores `StoredValue::EvmByteCode(ByteCode)`.
- `Key::EvmStorage(StorageAddr)` stores `StoredValue::EvmStorage(StorageValue)`.

An EVM account record contains:

- nonce,
- code hash,
- main purse.

Balances are Casper purse balances. EVM balance reads and writes reconcile
through the account main purse and `Key::Balance(main_purse.addr())`.

Genesis does not create EVM account records for Casper genesis accounts.
Funding an EVM identity is explicit: a native Casper transfer can use a
20-byte `evm::Address` as its `target` argument when `[evm].enabled = true`.
If `Key::EvmAccount(address)` already exists, the transfer credits that
account's main purse. If it does not exist, the transfer creates
`StoredValue::EvmAccount(Account::new(0, EMPTY_CODE_HASH,
evm::deterministic_purse(address)))`, initializes that deterministic purse
with a zero balance, then transfers the requested motes into it. Transfer
records keep the Casper transfer schema unchanged: `to` is `None`, and
`target` is the EVM account's backing purse.

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

`eth_call` uses a node binary-port command, not transaction submission.

The binary-port request carries:

- `from`,
- `to`,
- `value`,
- input bytes,
- gas limit.

Node handles the request only when speculative execution is enabled for the
binary port. Contract runtime checks out state at the requested/latest block,
runs `casper-executor-evm` with:

- `ExecuteKind::Call`,
- `CallValidation::UncheckedSimulation`,
- `FeeCharge::External`,

and returns output, status, and gas used. The tracking-copy effects are
discarded.

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
from this workspace.

From a separate `casper-devnet` checkout, register the asset with:

```bash
cd /path/to/casper-devnet
cargo run -- assets add evm \
    --casper-node "$CASPER_NODE_WORKSPACE/target/debug/casper-node" \
    --casper-sidecar "$CASPER_SIDECAR_WORKSPACE/target/debug/casper-sidecar" \
    --chainspec "$CASPER_NODE_WORKSPACE/resources/local/chainspec.toml" \
    --node-config "$CASPER_NODE_WORKSPACE/resources/local/config.toml" \
    --sidecar-config "$CASPER_SIDECAR_WORKSPACE/resources/example_configs/default_rpc_only_config.toml"
```

If `casper-devnet` is already installed on `PATH`, the equivalent command is:

```bash
casper-devnet assets add evm \
    --casper-node "$CASPER_NODE_WORKSPACE/target/debug/casper-node" \
    --casper-sidecar "$CASPER_SIDECAR_WORKSPACE/target/debug/casper-sidecar" \
    --chainspec "$CASPER_NODE_WORKSPACE/resources/local/chainspec.toml" \
    --node-config "$CASPER_NODE_WORKSPACE/resources/local/config.toml" \
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

### Fund EVM Identity

Create and fund the EVM identity explicitly with a native Casper transfer:

```bash
casper-cli transaction transfer \
    --from devnet:user-1 \
    --to 0x24790C4849cCAE43c0c1749e2C5b8d00Cc63AB80 \
    --amount 10000 \
    --raw \
    --no-interactive
```

The transfer target is encoded as `byte-array[20]`. A successful transfer
creates `Key::EvmAccount(0x24790c...)`, initializes its deterministic backing
purse, and credits it with the transferred motes. The EVM nonce remains `0x0`
until the first EVM transaction is executed.

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
Transaction hash: 0xa86146276e1cc132ddb750e9e053c3e7a1381222104cefb0c6c567556e5e9198
```

Forge may create local `cache/` and `out/` directories. They are build
artifacts and should not be committed.

The corresponding receipt should contain:

```text
status             0x1
contractAddress    0x6c0704679ca22b83778ef815607359cf6f5352b6
gasUsed            0x262ef
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
cast send 0x6c0704679CA22b83778Ef815607359cf6F5352B6 \
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
gasUsed              43803
to                   0x6c0704679CA22b83778Ef815607359cf6F5352B6
transactionHash      0x042ff975ec4b8fa8012f486bb7bd930e69978782b8b3c107ca2a276a43d7f293
```

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
cargo test -p casper-binary-port evm_call --lib
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
