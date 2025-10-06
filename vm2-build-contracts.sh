#!/bin/bash
set -e

VM2_BINS=(
  "vm2-harness"
  "vm2-cep18-caller"
  "vm2-system-caller"
)

VM2_LIBS=(
  "vm2-trait"
  "vm2-cep18"
  "vm2-flipper"
  "vm2-upgradable"
  "vm2-upgradable-v2"
  "vm2-vm1-wrapper"
  "vm2-host"
  "vm2-escrow"
  "vm2-named-args"
  "vm2-counter"
)


for contract in "${VM2_LIBS[@]}"
do
  pushd smart_contracts/contracts/vm2/$contract/
  pwd
  cargo build --target wasm32-unknown-unknown -p $contract --release
  popd
done

for contract in "${VM2_BINS[@]}"
do
  pushd smart_contracts/contracts/vm2/$contract/
  pwd
  cargo build --target wasm32-unknown-unknown -p $contract --release
  popd
done
