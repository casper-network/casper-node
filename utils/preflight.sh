#!/bin/sh

# THIS IS NOT AN "OFFICIAL" TOOL TO VALIDATE BUILDS, BUT A WAY TO SAVE SOME CI RUNTIME.

# Sample script that approximates a CI run. Can be run in the root directory to test the whole deal,
# or inside a crate folder for a partial (and thus quicker) check.
#
# It checks things that are cheap to check (formatting) or most likely to be missed (clippy) first,
# you will usually get good mileage out of the script by cancelling with early with C-c.

set -e

# Check formatting.
cargo fmt -- --check

# Run `cargo check`. Since Clippy will also essentially run `check`, this is done in a single step.
# cargo check --color=always --all-features
# Reset the timestamp on all source files because clippy won't pick them up otherwise.
find ./ -name \*.rs -and -not -path '*/target/*' -exec touch {} +
cargo clippy --color=always --all-targets

# Run the tests.
cargo test --color=always --all-features

# Build in release mode once for good measure.
cargo build --release --color=always --all-features
