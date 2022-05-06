#!/usr/bin/env bash

# Script used to group everything needed for nctl upgrade remotes.

set -e

trap clean_up EXIT

function clean_up() {
    local EXIT_CODE=$?

    if [ "$EXIT_CODE" = '0' ] && [ ! -z ${DRONE} ]; then
        # Running in CI so don't cleanup stage dir
        echo "Script completed successfully!"
        return
    fi

    if [ -d "$TEMP_STAGE_DIR" ]; then
        echo "Script exited $EXIT_CODE"
        echo "... Removing stage dir: $TEMP_STAGE_DIR"
        rm -rf "$TEMP_STAGE_DIR"
        exit "$EXIT_CODE"
    fi
}

# DIRECTORIES
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
BIN_BUILD_DIR="$ROOT_DIR/target/release"
WASM_BUILD_DIR="$ROOT_DIR/target/wasm32-unknown-unknown/release"
CONFIG_DIR="$ROOT_DIR/resources/local"
TEMP_STAGE_DIR='/tmp/nctl_upgrade_stage'
NCTL_UPGRADE_CHAINSPECS="$ROOT_DIR/utils/nctl/sh/scenarios/chainspecs"

# FILES
BIN_ARRAY=(casper-node)

WASM_ARRAY=(add_bid.wasm \
            delegate.wasm \
            transfer_to_account_u512.wasm \
            undelegate.wasm \
            withdraw_bid.wasm)

CONFIG_ARRAY=(chainspec.toml.in config.toml)

# Create temporary staging directory
if [ ! -d "$TEMP_STAGE_DIR" ]; then
    mkdir -p '/tmp/nctl_upgrade_stage'
fi

# Ensure files are built
cd "$ROOT_DIR"
cargo build --release --package casper-node
make build-contract-rs/activate-bid
make build-contract-rs/add-bid
make build-contract-rs/delegate
make build-contract-rs/named-purse-payment
make build-contract-rs/transfer-to-account-u512
make build-contract-rs/undelegate
make build-contract-rs/withdraw-bid

# Copy binaries to staging dir
for i in "${BIN_ARRAY[@]}"; do
    if [ -f "$BIN_BUILD_DIR/$i" ]; then
        echo "Copying $BIN_BUILD_DIR/$i to $TEMP_STAGE_DIR"
        cp "$BIN_BUILD_DIR/$i" "$TEMP_STAGE_DIR"
    else
        echo "ERROR: $BIN_BUILD_DIR/$i not found!"
        exit 1
    fi
    echo ""
done

# Copy wasm to staging dir
for i in "${WASM_ARRAY[@]}"; do
    if [ -f "$WASM_BUILD_DIR/$i" ]; then
        echo "Copying $WASM_BUILD_DIR/$i to $TEMP_STAGE_DIR"
        cp "$WASM_BUILD_DIR/$i" "$TEMP_STAGE_DIR"
    else
        echo "ERROR: $WASM_BUILD_DIR/$i not found!"
        exit 2
    fi
    echo ""
done

# Copy configs to staging dir
for i in "${CONFIG_ARRAY[@]}"; do
    if [ -f "$CONFIG_DIR/$i" ]; then
        echo "Copying $CONFIG_DIR/$i to $TEMP_STAGE_DIR"
        cp "$CONFIG_DIR/$i" "$TEMP_STAGE_DIR"
    else
        echo "ERROR: $CONFIG_DIR/$i not found!"
        exit 3
    fi
    echo ""
done

# Copy NCTL upgrade chainspecs
pushd "$NCTL_UPGRADE_CHAINSPECS"
cp upgrade* "$TEMP_STAGE_DIR"
popd
