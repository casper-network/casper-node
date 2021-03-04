#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

# Exit if any of the commands fail.
set -e

function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: Bonding test"
    log "------------------------------------------------------------"

    # 0. Wait for network to start up
    do_await_genesis_era_to_complete
    # 1. Allow the chain to progress
    do_await_era_change 1
    # 2. Verify all nodes are in sync
    check_network_sync
    # 3. Submit bid for node 6
    do_submit_auction_bids "6"
    # 4. wait auction_delay + 1
    do_await_era_change "1"
    # 5. Start node 6
    do_read_lfb_hash "5"
    do_start_node "6" "$LFB_HASH"
    # 6. Allow for the chain to progress further.
    do_await_era_change "2"
    # 7. Assert that the validator is bonded in.
    assert_new_bonded_validator "6"

    log "------------------------------------------------------------"
    log "Scenario bonding complete"
    log "------------------------------------------------------------"

}

function log_step() {
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    STEP=$((STEP + 1))
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

function do_await_genesis_era_to_complete() {
    log_step "awaiting genesis era to complete"
    ERA_ID=1
    while [ "$(get_chain_era)" -lt "$ERA_ID" ]; do
        sleep 1.0
    done
}

function do_await_eras_to_bid() {
    log_step "Presenting a buffer of one era"
    await_n_eras 1
}

function do_read_lfb_hash() {
    local NODE_ID=${1}
    LFB_HASH=$(render_last_finalized_block_hash "$NODE_ID" | cut -f2 -d= | cut -f2 -d ' ')
    echo "$LFB_HASH"
}

function do_submit_auction_bids()
{
    local NODE_ID=${1}
    log_step "submitting POS auction bids:"
    log "----- ----- ----- ----- ----- -----"
    BID_AMOUNT=$(get_node_staking_weight "$NODE_ID")
    BID_DELEGATION_RATE=2

    source "$NCTL"/sh/contracts-auction/do_bid.sh \
            node="$NODE_ID" \
            amount="$BID_AMOUNT" \
            rate="$BID_DELEGATION_RATE" \
            quiet="TRUE"

    log "node-$NODE_ID auction bid submitted -> $BID_AMOUNT CSPR"

    log_step "awaiting 10 seconds for auction bid deploys to finalise"
    sleep 10.0
}

function do_start_node() {
    local NODE_ID=${1}
    local LFB_HASH=${2}
    log_step "starting node-$NODE_ID. Syncing from hash=${LFB_HASH}"
    do_node_start "$NODE_ID" "$LFB_HASH"
    sleep 1
    if [ "$(do_node_status ${NODE_ID} | awk '{ print $2 }')" != "RUNNING" ]; then
        log "ERROR: node-${NODE_ID} is not running"
	exit 1
    else
        log "node-${NODE_ID} is running"
    fi
}

function get_node_public_key_hex() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(cat "$NODE_PATH"/keys/public_key_hex)
    echo "$PUBLIC_KEY_HEX"
}

function check_current_era {
    echo $(nctl-view-chain-era node=$(get_node_for_dispatch) | awk '{print $NF}')
}


function assert_new_bonded_validator() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex "$NODE_ID")
    if ! $(nctl-view-chain-auction-info | grep -q "$HEX"); then
      echo "Could not find key in bids"
      exit 1
    fi
}


function do_await_era_change() {
    # allow chain height to grow
    local ERA_COUNT=${1:-"1"}
    log_step "awaiting $ERA_COUNT eras…"
    await_n_eras "$ERA_COUNT"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

STEP=0

main
