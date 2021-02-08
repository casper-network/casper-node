#!/bin/sh

set -u

# Settings
NODE_ADDRESS=http://localhost:7777
SECOND_NODE_ADDRESS=http://localhost:7778

# CLIENT="cargo run --manifest-path=../client/Cargo.toml --"
CLIENT=../target/debug/casper-client
CHAIN_NAME=mynet
NODE_IDX=2

# Caluclated values
SECRET_KEY_PATH=${CHAIN_NAME}/node-${NODE_IDX}/keys/secret_key.pem
WASM_DIR=$(readlink -f ../target/wasm32-unknown-unknown/release/)

# Helper function to check a deploy exists.
deploy_exists() {
  ADDRESS=$1
  HASH=$2
  SHORT_HASH=$(echo $HASH | cut -c 1-8)

  if ${CLIENT} get-deploy --node-address ${ADDRESS} ${HASH} > /dev/null; then
    echo "Deploy ${SHORT_HASH} found on ${ADDRESS}"
  else
    echo "Deploy ${SHORT_HASH} NOT FOUND on ${ADDRESS}"
  fi;
}

DEPLOY_HASH=$(${CLIENT} put-deploy \
  --chain-name ${CHAIN_NAME}\
  --node-address ${NODE_ADDRESS}\
  --secret-key "${SECRET_KEY_PATH}" \
  --session-path "${WASM_DIR}/do_nothing.wasm" \
  --payment-amount 10000000000 | tee /dev/tty | jq -r '.result.deploy_hash' )

sleep 0.5;

deploy_exists ${NODE_ADDRESS} ${DEPLOY_HASH}
deploy_exists ${SECOND_NODE_ADDRESS} ${DEPLOY_HASH}
