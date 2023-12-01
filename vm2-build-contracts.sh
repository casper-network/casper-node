#!/bin/bash
set -e
VM2_CONTRACTS=(
  "vm2-test-contract"
  "vm2-harness"
)
for contract in "${VM2_CONTRACTS[@]}"
do
  pushd smart_contracts/contracts/vm2/$contract/
  pwd
  RUSTFLAGS="-g -Z macro-backtrace" cargo build --target wasm32-unknown-unknown --release
  popd
done
