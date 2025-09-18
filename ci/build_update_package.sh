#!/usr/bin/env bash

# This script will build
#  - bin.tar.gz
#  - config.tar.gz (and additional ones for each network directory in resources)
#  - version.json
# in target/upgrade_build

set -e

if command -v jq >&2; then
  echo "jq installed"
else
  echo "ERROR: jq is not installed and required"
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
LATEST_DIR="$ROOT_DIR/target/latest"
GENESIS_FILES_DIR="$ROOT_DIR/resources"
PRODUCTION_GENESIS_FILES_DIR="$GENESIS_FILES_DIR/production"
MAIN_GENESIS_FILES_DIR="$GENESIS_FILES_DIR/mainnet"
TEST_GENESIS_FILES_DIR="$GENESIS_FILES_DIR/testnet"
INT_GENESIS_FILES_DIR="$GENESIS_FILES_DIR/integration-test"
DEV_GENESIS_FILES_DIR="$GENESIS_FILES_DIR/dev-net"
NODE_BUILD_TARGET="$ROOT_DIR/target/release/casper-node"
NODE_BUILD_DIR="$ROOT_DIR/node"
UPGRADE_DIR="$ROOT_DIR/target/upgrade_build/"
BIN_DIR="$UPGRADE_DIR/bin"
CONFIG_DIR="$UPGRADE_DIR/config"

GIT_HASH=$(git rev-parse HEAD)
BRANCH_NAME=$(git branch --show-current)
PROTOCOL_VERSION=$(cat "$PRODUCTION_GENESIS_FILES_DIR/chainspec.toml" | python3 -c "import sys, toml; print(toml.load(sys.stdin)['protocol']['version'].replace('.','_'))")
NODE_VERSION=$(cat "$NODE_BUILD_DIR/Cargo.toml" | python3 -c "import sys, toml; print(toml.load(sys.stdin)['package']['version'])")

echo "Creating $BRANCH_NAME.latest file"
mkdir -p "$LATEST_DIR"
echo -n "$GIT_HASH" > "$LATEST_DIR/$BRANCH_NAME.latest"

echo "Building casper-node"
cd "$NODE_BUILD_DIR" || exit
cargo build --release

echo "Building global-state-update-gen"
cd "$ROOT_DIR" || exit
cargo build --release --package global-state-update-gen
cargo deb --package global-state-update-gen
mkdir -p "$UPGRADE_DIR"
cp "$ROOT_DIR/target/debian/"* "$UPGRADE_DIR" || exit

echo "Generating bin README.md"
mkdir -p "$BIN_DIR"
readme="$BIN_DIR/README.md"
{
  echo "Build for Ubuntu 22.04."
  echo ""
  echo "To run on other platforms, build from https://github.com/casper-network/casper-node"
  echo " cd node"
  echo " cargo build --release"
  echo ""
  echo "git commit hash: $GIT_HASH"
} > "$readme"

echo "Packaging bin.tar.gz"
mkdir -p "$BIN_DIR"
cp "$NODE_BUILD_TARGET" "$BIN_DIR"
# To get no path in tar, need to cd in.
cd "$BIN_DIR" || exit
tar -czvf "../bin.tar.gz" .
cd ..
rm -rf "$BIN_DIR"

echo "Packaging config.tar.gz"
mkdir -p "$CONFIG_DIR"
cp "$PRODUCTION_GENESIS_FILES_DIR/chainspec.toml" "$CONFIG_DIR"
cp "$PRODUCTION_GENESIS_FILES_DIR/config-example.toml" "$CONFIG_DIR"
cp "$PRODUCTION_GENESIS_FILES_DIR/accounts.toml" "$CONFIG_DIR"
# To get no path in tar, need to cd in.
cd "$CONFIG_DIR" || exit
tar -czvf "../config.tar.gz" .
cd ..
rm -rf "$CONFIG_DIR"

echo "Packaging config-main.tar.gz"
mkdir -p "$CONFIG_DIR"
cp "$MAIN_GENESIS_FILES_DIR/chainspec.toml" "$CONFIG_DIR"
cp "$MAIN_GENESIS_FILES_DIR/config-example.toml" "$CONFIG_DIR"
# To get no path in tar, need to cd in.
cd "$CONFIG_DIR" || exit
tar -czvf "../config-main.tar.gz" .
cd ..
rm -rf "$CONFIG_DIR"

echo "Packaging config-test.tar.gz"
mkdir -p "$CONFIG_DIR"
cp "$TEST_GENESIS_FILES_DIR/chainspec.toml" "$CONFIG_DIR"
cp "$TEST_GENESIS_FILES_DIR/config-example.toml" "$CONFIG_DIR"
# To get no path in tar, need to cd in.
cd "$CONFIG_DIR" || exit
tar -czvf "../config-test.tar.gz" .
cd ..
rm -rf "$CONFIG_DIR"

echo "Packaging config-int.tar.gz"
mkdir -p "$CONFIG_DIR"
cp "$INT_GENESIS_FILES_DIR/chainspec.toml" "$CONFIG_DIR"
cp "$INT_GENESIS_FILES_DIR/config-example.toml" "$CONFIG_DIR"
# To get no path in tar, need to cd in.
cd "$CONFIG_DIR" || exit
tar -czvf "../config-int.tar.gz" .
cd ..
rm -rf "$CONFIG_DIR"

echo "Packaging config-dev.tar.gz"
mkdir -p "$DEV_CONFIG_DIR"
cp "$DEV_GENESIS_FILES_DIR/chainspec.toml" "$CONFIG_DIR"
cp "$DEV_GENESIS_FILES_DIR/config-example.toml" "$CONFIG_DIR"
# To get no path in tar, need to cd in.
cd "$CONFIG_DIR" || exit
tar -czvf "../config-dev.tar.gz" .
cd ..
rm -rf "$CONFIG_DIR"

echo "Building version.json"
jq --null-input \
--arg	branch "$BRANCH_NAME" \
--arg version "$NODE_VERSION" \
--arg pv "$PROTOCOL_VERSION" \
--arg ghash "$GIT_HASH" \
--arg now "$(jq -nr 'now | strftime("%Y-%m-%dT%H:%M:%SZ")')" \
--arg files "$(ls "$UPGRADE_DIR" | jq -nRc '[inputs]')" \
'{"branch": $branch, "version": $version, "protocol_version": $pv, "git-hash": $ghash, "timestamp": $now, "files": $files}' \
> "$UPGRADE_DIR/version.json"
