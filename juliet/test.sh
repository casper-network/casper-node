#!/bin/sh

#: Shorthand script to run test with logging setup correctly.

RUST_LOG=${RUST_LOG:-juliet=trace}
export RUST_LOG

# Run one thread at a time to not get interleaved output.
exec cargo test --features tracing -- --test-threads=1 --nocapture $@
