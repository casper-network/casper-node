VM2_CONTRACTS=(
  "vm2-test-contract"
)
for contract in "${VM2_CONTRACTS[@]}"
do
  pushd smart_contracts/contracts/vm2/$contract/
  pwd
  RUSTFLAGS=-g cargo build --target wasm32-unknown-unknown --release
  popd
done
