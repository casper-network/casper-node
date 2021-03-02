#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration test that tries to simulate
# and verify validator ejection.
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: itst13"
    log "------------------------------------------------------------"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Allow chain to progress
    do_await_era_change
    # 2. Verify all nodes are in sync
    check_network_sync
    # 3. Stop the node
    do_stop_node '5'
    # 4. wait auction_delay + 1
    do_await_era_change '4'
    # 5. Validate eviction occured
    assert_eviction '5'
    log "------------------------------------------------------------"
    log "Scenario itst13 complete"
    log "------------------------------------------------------------"
}

function log_step() {
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    STEP=$((STEP + 1))
}

function do_await_genesis_era_to_complete() {
    log_step "awaiting genesis era to complete"
    while [ "$(get_chain_era)" -lt 1 ]; do
        sleep 1.0
    done
}

function do_read_lfb_hash() {
    local NODE_ID=${1}
    LFB_HASH=$(render_last_finalized_block_hash "$NODE_ID" | cut -f2 -d= | cut -f2 -d ' ')
    echo "$LFB_HASH"
}

function do_stop_node() {
    local NODE_ID=${1}
    log_step "stopping node-$NODE_ID."
    do_node_stop "$NODE_ID"
    sleep 1
    if [ "$(do_node_status ${NODE_ID} | awk '{ print $2 }')" = "RUNNING" ]; then
        log "ERROR: node-${NODE_ID} is still running"
	exit 1
    else
        log "node-${NODE_ID} was shutdown in era: $(check_current_era)"
    fi
}

function check_network_sync() {
    local WAIT_TIME_SEC=0
    log_step "check all nodes are in sync"
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if [ "$(do_read_lfb_hash '5')" = "$(do_read_lfb_hash '1')" ] && \
		[ "$(do_read_lfb_hash '4')" = "$(do_read_lfb_hash '1')" ] && \
		[ "$(do_read_lfb_hash '3')" = "$(do_read_lfb_hash '1')" ] && \
		[ "$(do_read_lfb_hash '2')" = "$(do_read_lfb_hash '1')" ]; then
	    log "all nodes in sync, proceeding..."
	    break
        fi

        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to confirm network sync"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1
    done
}

function do_await_era_change() {
    # allow chain height to grow
    local ERA_COUNT=${1:-"1"}
    log_step "awaiting $ERA_COUNT eras…"
    await_n_eras "$ERA_COUNT"
}

# Checks what the current era is
function check_current_era {
    echo $(nctl-view-chain-era node=$(get_node_for_dispatch) | awk '{print $NF}')
}

# Checks that a validator gets marked as inactive
function check_inactive() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex "$NODE_ID")
    nctl-view-chain-auction-info | jq --arg node_hex "$HEX" '.auction_state.bids[] | select(.public_key == $node_hex).bid.inactive' | grep -q 'true'
    return $?
}

# Captures the public key in hex of a node
function get_node_public_key_hex() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(cat "$NODE_PATH"/keys/public_key_hex)
    echo "$PUBLIC_KEY_HEX"
}

# Checks that the current era + 1 contains a nodes 
# public key hex
function check_removed_validator() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex "$NODE_ID")
    local ERA=$(check_current_era)
    # Plus 1 to avoid query issue if era switches mid run
    local ERA_PLUS_1=$(expr $ERA + 1)
    # note: tonumber is a must here to prevent jq from being to smart
    nctl-view-chain-auction-info | jq --arg era "$ERA_PLUS_1" '.auction_state.era_validators[] | select(.era_id == ($era | tonumber))' | grep -q "$HEX"
    return $?
}

function assert_eviction() {
    local NODE_ID=${1}
    log_step "Checking for evicted node-$NODE_ID..."
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if ( ! check_removed_validator "$NODE_ID" ) && ( check_inactive "$NODE_ID" ); then
            log "validator node-$NODE_ID was ejected! [expected]"
            break
        fi

        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to confirm a faulty validator"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
STEP=0

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        timeout) SYNC_TIMEOUT_SEC=${VALUE} ;;
        *) ;;
    esac
done

SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}

main
