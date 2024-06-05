#!/bin/bash
set -e

VM2_BINS=(
  #"vm2-test-contract"
  "vm2-harness"
  "vm2-cep18-caller"
)

VM2_LIBS=(
  "vm2-trait"
  "vm2-cep18"
  "vm2-flipper"
  "vm2-upgradable"
)

for contract in "${VM2_BINS[@]}"
do
  pushd smart_contracts/contracts/vm2/$contract/
  pwd
  cargo build --target wasm32-unknown-unknown -p $contract --bin $contract --release
  popd
done

for contract in "${VM2_LIBS[@]}"
do
  pushd smart_contracts/contracts/vm2/$contract/
  pwd
  cargo build --target wasm32-unknown-unknown -p $contract --lib --release
  popd
done
