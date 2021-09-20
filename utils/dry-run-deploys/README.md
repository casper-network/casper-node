# `dry-run-deploys` 

Run deploys against a local `casper-execution-engine`.

Use example:
```bash
# First, generate chain-download and lmdb-data directories, containing the 
# blocks in the chain and the trie respectively
cd ../retrieve-state
cargo run -- -a download-blocks # download all blocks from a given network
cargo run -- -a download-tries -h 0 # Download the trie at genesis

cd ../dry-run-deploys

# The tries currently downloaded represent height 0, so execute blocks from height 
# 1 to 10 (inclusive)
cargo run -- --chain-download-path ../retrieve-state/chain-download \
  --lmdb-path ../retrieve-state/lmdb-data \
  --starting-block-height 1 \
  --ending-block-height 10
```