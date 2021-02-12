#!/usr/bin/env bash
set -e

# This script is used to test install of casper-node.
# It will also detect issues with chainspec.toml and config-example.toml.
# Often, new settings for casper-node get missed in resources/production directory
# as most developers test using resources/local.  This will display any crashes from
# configuration in CI, rather than requiring manual work with a release.

DEB_NAME="casper-node"

echo "$1/$DEB_NAME*.deb"
apt-get install -y "$1"/target/debian/"$DEB_NAME"*.deb

if ! type "$DEB_NAME" > /dev/null; then
  exit 1
fi

cp "$1/resources/production/chainspec.toml" /etc/casper/chainspec.toml

# Replace timestamp with future time in chainspec.toml to not get start after genesis error
FUTURE_TIME=$(date -d '+1 hour' --utc +%FT%TZ)
sed -i "/timestamp =/c\timestamp = \'$FUTURE_TIME\'" /etc/casper/chainspec.toml

TEST_RUN_OUTPUT="$1/casper_node_run_output"
# This will fail for no keys, but will fail for config.toml or chainspec.toml first
casper-node validator /etc/casper/config.toml &> "$TEST_RUN_OUTPUT" || true

apt-get remove -y "$DEB_NAME"

EXPECTED_TEXT="secret key load failed: could not read '/etc/casper/validator_keys/secret_key.pem'"
if grep < "$TEST_RUN_OUTPUT" -q "$EXPECTED_TEXT"; then
    echo "Found key failures as expected"
else
    echo "#################################"
    echo "Expected key failures, not found."
    echo "Assume this is configuration related for config-example.toml or chainspec.toml in resources/production."
    echo "Run log:"
    cat "$TEST_RUN_OUTPUT"
    exit 1
fi
