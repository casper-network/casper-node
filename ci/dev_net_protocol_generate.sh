#!/usr/bin/env bash

# Need IP for
NODE_RPC_URL="https://node-1.dev.casper.network/rpc"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
CI_SCRIPT_DIR="$ROOT_DIR/ci"
TARGET_DIR="$ROOT_DIR/target"
GENESIS_DIR="$TARGET_DIR/genesis"
CONFIG_DIR="$TARGET_DIR/config"

# pull latest dev hash from artifacts
CURRENT_HASH=$(curl -s https://genesis.casper.network/artifacts/casper-node/dev.latest)
echo "Checked out Github hash $CURRENT_HASH"

LATEST_HASH=$(curl -s https://genesis.casper.network/dev-net/latest_git_hash | tr -d '\n')
echo "Latest Hash from dev-net protocol is $LATEST_HASH"

echo

if [ "$CURRENT_HASH" == "$LATEST_HASH" ]; then
	  echo "Last published dev-net protocol has same hash, erroring out."
	  exit 1 # This fails job and stops workflow
fi

LATEST_PROTOCOL_VERSION="$(curl -s https://genesis.casper.network/dev-net/protocol_versions | tail -n 1 | tr -d '\n')"
echo "Latest dev-net protocol version: $LATEST_PROTOCOL_VERSION"

IFS="_"
# Read latest protocol parts into array
read -ra LPVA <<< "$LATEST_PROTOCOL_VERSION"

# Incrementing one to patch
NEW_PROTOCOL_VERSION=${LPVA[0]}_${LPVA[1]}_$((LPVA[2] + 1))
echo "New dev-net protocol version: $NEW_PROTOCOL_VERSION"
echo

PROTOCOL_DIR="$GENESIS_DIR/$NEW_PROTOCOL_VERSION"
echo "## Creating $PROTOCOL_DIR"
mkdir -p "$PROTOCOL_DIR"
echo

echo "$NEW_PROTOCOL_VERSION" > "$GENESIS_DIR/protocol_versions"
echo "## protocol_versions file contents:"
echo "---"
cat "$GENESIS_DIR/protocol_versions"
echo "---"
echo

echo "$CURRENT_HASH" > "$GENESIS_DIR/latest_git_hash"
echo "## latest_git_hash file contents:"
echo "---"
cat "$GENESIS_DIR/latest_git_hash"
echo "---"
echo

mkdir -p "$CONFIG_DIR"
cd "$TARGET_DIR" || exit 1
DOWNLOAD_PATH="https://genesis.casper.network/artifacts/casper-node/$CURRENT_HASH"
echo "## Downloading: $DOWNLOAD_PATH/bin.tar.gz"
curl -JLO "$DOWNLOAD_PATH/bin.tar.gz" || exit 1

echo "## Downloading: $DOWNLOAD_PATH/config-dev.tar.gz"
curl -JLO "$DOWNLOAD_PATH/config-dev.tar.gz" || exit 1

cd "$CONFIG_DIR" || exit 1
# This will validate that files retrieved were good. Should error if just curl output

echo "## Decompressing config"
tar -xzvf ../config-dev.tar.gz . || exit 1

ACTIVATION_POINT=$("$CI_SCRIPT_DIR/next_upgrade_era_with_buffer.sh" "$NODE_RPC_URL" 15)

echo "## Replacing activation_point in chainspec.toml with $ACTIVATION_POINT"
# chainspec.toml replacement
sed -i '/^activation_point = /c\activation_point = '"$ACTIVATION_POINT" chainspec.toml

# config_example.toml replacement
echo "## Retrieving statuses to make known_addresses"
KNOWN_ADDRESSES="[$( (curl -s https://node-1.dev.casper.network/status | jq -r '.peers[] | .address';
                      curl -s https://node-2.dev.casper.network/status | jq -r '.peers[] | .address';
                      curl -s https://node-3.dev.casper.network/status | jq -r '.peers[] | .address';
                      curl -s https://node-4.dev.casper.network/status | jq -r '.peers[] | .address';) |
                      sort | uniq | xargs -d '\n' printf "'%s'," | sed 's/, $//' )]"
echo "## Replacing known_addresses in config-example.toml with $KNOWN_ADDRESSES"
sed -i '/^known_addresses = /c\known_addresses = '"$KNOWN_ADDRESSES" config-example.toml

CS_PROTOCOL=$(echo -n "$NEW_PROTOCOL_VERSION" | tr '_' '.')
echo "## Replacing protocol.version with $CS_PROTOCOL"
sed -i '/^version = /c\version = '\'"$CS_PROTOCOL"\' chainspec.toml

echo "## Compressing new config.tar.gz"
tar -czvf ../config.tar.gz .

cd ..
pwd

echo "## Moving files bin and config into $PROTOCOL_DIR"
mv config.tar.gz "$PROTOCOL_DIR/"
mv bin.tar.gz "$PROTOCOL_DIR/"

echo "## Listing contents of $GENESIS_DIR"
ls -alrR "$GENESIS_DIR"
