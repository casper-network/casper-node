#!/bin/bash
set -e
VM2_CONTRACTS=(
  #"vm2-test-contract"
  "vm2-harness"
  #"vm2-cep18"
  #"vm2-cep18-caller"
  "vm2-trait"
)
for contract in "${VM2_CONTRACTS[@]}"
do
  pushd smart_contracts/contracts/vm2/$contract/
  pwd
  cargo build --target wasm32-unknown-unknown -p $contract --lib --release
  popd
done
