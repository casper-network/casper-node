#!/usr/bin/env bash
set -e

DRONE_ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
SCENARIOS_DIR="$DRONE_ROOT_DIR/utils/nctl/sh/scenarios"
SCENARIOS_CHAINSPEC_DIR="$SCENARIOS_DIR/chainspecs"
SCENARIOS_ACCOUNTS_DIR="$SCENARIOS_DIR/accounts_toml"
SCENARIOS_CONFIGS_DIR="$SCENARIOS_DIR/configs"
LAUNCHER_DIR="$DRONE_ROOT_DIR/.."

# NCTL requires casper-node-launcher
if [ ! -d "$LAUNCHER_DIR/casper-node-launcher" ]; then
    pushd "$LAUNCHER_DIR"
    git clone https://github.com/CasperLabs/casper-node-launcher.git
fi

# Activate Environment
pushd "$DRONE_ROOT_DIR"
source "$(pwd)"/utils/nctl/activate

# Build, Setup, and Start NCTL
nctl-compile

function start_run_teardown() {
    local RUN_CMD=$1
    local TEST_NAME
    local SETUP_ARGS

    # Capture test prefix for custom file checks
    TEST_NAME="$(echo $RUN_CMD | awk -F'.sh' '{ print $1 }')"

    # Really-really make sure nothing is leftover
    nctl-assets-teardown

    # Check for custom chainspec
    if [ -f "$SCENARIOS_CHAINSPEC_DIR/$TEST_NAME.chainspec.toml.in" ]; then
        SETUP_ARGS+=("chainspec_path=$SCENARIOS_CHAINSPEC_DIR/$TEST_NAME.chainspec.toml.in")
    fi

    # Check for custom accounts
    if [ -f "$SCENARIOS_ACCOUNTS_DIR/$TEST_NAME.accounts.toml" ]; then
        SETUP_ARGS+=("accounts_path=$SCENARIOS_ACCOUNTS_DIR/$TEST_NAME.accounts.toml")
    fi

    # Check for custom config
    if [ -f "$SCENARIOS_CONFIGS_DIR/$TEST_NAME.config.toml" ]; then
        SETUP_ARGS+=("config_path=$SCENARIOS_CONFIGS_DIR/$TEST_NAME.config.toml")
    fi

    # Setup nctl files for test
    echo "Setting up network: nctl-assets-setup ${SETUP_ARGS[@]}"
    nctl-assets-setup "${SETUP_ARGS[@]}"
    sleep 1

    # Start nctl network
    nctl-start
    echo "Sleeping 90 to allow network startup"
    sleep 90

    # Run passed in test
    pushd "$SCENARIOS_DIR"
    echo "Starting scenario: $RUN_CMD"
    # Don't qoute the cmd
    source $RUN_CMD

    # Cleanup after test completion
    popd
    nctl-assets-teardown
    sleep 1
}

start_run_teardown "itst01.sh"
start_run_teardown "itst02.sh"
start_run_teardown "itst06.sh"
start_run_teardown "itst07.sh"
start_run_teardown "itst11.sh"
start_run_teardown "itst13.sh"
start_run_teardown "itst14.sh"
start_run_teardown "bond_its.sh"
start_run_teardown "emergency_upgrade_test.sh"
start_run_teardown "emergency_upgrade_test_balances.sh"
start_run_teardown "sync_test.sh node=6 timeout=500"
# Keep this test last
start_run_teardown "sync_upgrade_test.sh node=6 era=5 timeout=500"
