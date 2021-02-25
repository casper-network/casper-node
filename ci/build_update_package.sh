#!/usr/bin/env bash

# This script will build bin.tar.gz and config.tar.gz in target/upgrade_build

set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
NODE_BUILD_TARGET="$ROOT_DIR/target/release/casper-node"
UPGRADE_DIR="$ROOT_DIR/target/upgrade_build"
BIN_DIR="$UPGRADE_DIR/bin"
CONFIG_DIR="$UPGRADE_DIR/config"
NODE_BUILD_DIR="$ROOT_DIR/node"
GENESIS_FILES_DIR="$ROOT_DIR/resources/production"

echo "Building casper-node"
cd "$NODE_BUILD_DIR"
cargo build --release

echo "Generating bin README.md"
mkdir -p "$BIN_DIR"
readme="$BIN_DIR/README.md"
{
  echo "Build for Ubuntu 18.04."
  echo ""
  echo "To run on other platforms, build from https://github.com/CasperLabs/casper-node"
  echo " cd node"
  echo " cargo build --release"
  echo ""
  echo "git commit hash: $(git rev-parse HEAD)"
} > "$readme"

echo "Packaging bin.tar.gz"
mkdir -p "$BIN_DIR"
cp "$NODE_BUILD_TARGET" "$BIN_DIR"
# To get no path in tar, need to cd in.
cd "$BIN_DIR"
tar -czvf "../bin.tar.gz" .
cd ..
rm -rf "$BIN_DIR"

echo "Packaging config.tar.gz"
mkdir -p "$CONFIG_DIR"
cp "$GENESIS_FILES_DIR/chainspec.toml" "$CONFIG_DIR"
cp "$GENESIS_FILES_DIR/config-example.toml" "$CONFIG_DIR"
cp "$GENESIS_FILES_DIR/accounts.toml" "$CONFIG_DIR"
# To get no path in tar, need to cd in.
cd "$CONFIG_DIR"
tar -czvf "../config.tar.gz" .
cd ..
rm -rf "$CONFIG_DIR"
