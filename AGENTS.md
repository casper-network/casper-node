# AGENTS.md

- Use `make lint` for linting; do not run `cargo clippy` directly.
- Use `make format` for formatting; do not run `cargo fmt` directly.
- Prefer targeted tests for touched crates or modules when verifying changes.
- Keep `resources/local/chainspec.toml.in` in sync when editing chainspecs; run `./generate-chainspec.sh` when `resources/local/chainspec.toml` is missing or stale.
