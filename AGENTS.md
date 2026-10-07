# AGENTS.md

- Use `make lint` for linting; do not run `cargo clippy` directly.
- Use `make format` for formatting; do not run `cargo fmt` directly.
- Prefer targeted tests for touched crates or modules when verifying changes.
- If EVM test fixtures under `target/evm-contracts/*.bin` are missing, run `make build-contracts-evm`.
- If Wasm contract fixtures are missing, run `make build-contracts-rs`.
- Keep `resources/local/chainspec.toml.in` in sync when editing chainspecs; run `./generate-chainspec.sh` when `resources/local/chainspec.toml` is missing or stale.
- Treat idempotent system contract/predeploy upserts in protocol upgrade handlers as standard activation behavior, not as an alternative to `global_state_update`.
- Do not turn malformed or invalid network-reachable input, including fetched transactions and proposed blocks, into a fatal reactor error. Reject it or record a per-item failure; reserve fatal announcements for internal invariants or unrecoverable local state.
