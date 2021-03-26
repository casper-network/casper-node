#!/usr/bin/env bash
set -e

# Meant to run only in CI
if [ -z "${DRONE}" ]; then
  echo "Must be run on Drone!"
  exit 1
fi

DRONE_ROOT_DIR="/drone/src"
SCENARIOS_DIR="$DRONE_ROOT_DIR/utils/nctl/sh/scenarios"
SCENARIOS_CHAINSPEC_DIR="$SCENARIOS_DIR/chainspecs"
SCENARIOS_ACCOUNTS_DIR="$SCENARIOS_DIR/accounts_toml"
LAUNCHER_DIR="/drone"

# NCTL requires casper-node-launcher
pushd $LAUNCHER_DIR
git clone https://github.com/CasperLabs/casper-node-launcher.git

# Activate Environment
pushd $DRONE_ROOT_DIR
source $(pwd)/utils/nctl/activate
# Build, Setup, and Start NCTL
nctl-compile

function start_run_teardown() {
    local RUN_CMD=$1
    local RUN_CHAINSPEC=$2
    local RUN_ACCOUNTS=$3
    # Really-really make sure nothing is leftover
    nctl-assets-teardown
    echo "Setting up network: $RUN_CMD $RUN_CHAINSPEC $RUN_ACCOUNTS"
    if [ -z "$RUN_CHAINSPEC" ] && [ -z "$RUN_ACCOUNTS" ]; then
        nctl-assets-setup
    elif [ ! -z "$RUN_CHAINSPEC" ] && [ -z "$RUN_ACCOUNTS" ]; then
        nctl-assets-setup chainspec_path="$SCENARIOS_CHAINSPEC_DIR/$RUN_CHAINSPEC"
    elif [ -z "$RUN_CHAINSPEC" ] && [ ! -z "$RUN_ACCOUNTS" ]; then
        nctl-assets-setup accounts_path="$SCENARIOS_ACCOUNTS_DIR/$RUN_ACCOUNTS"
    else
        nctl-assets-setup chainspec_path="$SCENARIOS_CHAINSPEC_DIR/$RUN_CHAINSPEC" accounts_path="$SCENARIOS_ACCOUNTS_DIR/$RUN_ACCOUNTS"
    fi
    sleep 1
    nctl-start
    echo "Sleeping 90 to allow network startup"
    sleep 90
    pushd $SCENARIOS_DIR
    # Don't qoute the cmd
    echo "Starting scenario: $RUN_CMD $RUN_CHAINSPEC $RUN_ACCOUNTS"
    source $RUN_CMD
    popd
    nctl-assets-teardown
    sleep 1
}

start_run_teardown "itst01.sh"
start_run_teardown "itst02.sh"
start_run_teardown "itst11.sh"
start_run_teardown "itst13.sh" "itst13.chainspec.toml.in"
start_run_teardown "itst14.sh" "itst14.chainspec.toml.in" "itst14.accounts.toml"
start_run_teardown "sync_test.sh node=6 timeout=500"
start_run_teardown "sync_upgrade_test.sh node=6 era=4 timeout=500"
start_run_teardown "bond_its.sh" "bond_its.chainspec.toml.in" "bond_its.accounts.toml"

# Clean up cloned repo
popd
echo "Removing $LAUNCHER_DIR/casper-node-launcher"
rm -rf "$LAUNCHER_DIR/casper-node-launcher"
