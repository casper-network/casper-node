#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh

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
    do_read_lfb_hash "5"
    do_start_node "6" "$LFB_HASH"
    # 4. wait auction_delay + 1
    do_await_era_change "3"
    # 5. Assert that the validator is bonded in.
    assert_new_bonded_validator "6"
    log "The new node has bonded in."
    # 6. Assert that the new bonded validator is producing blocks.
    assert_node_proposed "6"


    log "------------------------------------------------------------"
    log "Scenario bonding complete"
    log "------------------------------------------------------------"

}

function do_submit_auction_bids()
{
    local NODE_ID=${1}
    log_step "submitting POS auction bids:"
    log "----- ----- ----- ----- ----- -----"
    BID_AMOUNT=$(get_node_staking_weight "$NODE_ID")
    BID_DELEGATION_RATE=6

    source "$NCTL"/sh/contracts-auction/do_bid.sh \
            node="$NODE_ID" \
            amount="$BID_AMOUNT" \
            rate="$BID_DELEGATION_RATE" \
            quiet="TRUE"

    log "node-$NODE_ID auction bid submitted -> $BID_AMOUNT CSPR"

    log_step "awaiting 10 seconds for auction bid deploys to finalise"
    sleep 10.0
}


function assert_new_bonded_validator() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex "$NODE_ID")
    if ! $(nctl-view-chain-auction-info | grep -q "$HEX"); then
      echo "Could not find key in bids"
      exit 1
    fi
}

function assert_node_proposed() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(get_node_public_key_hex $NODE_ID)
    log_step "Waiting for node-$NODE_ID to produce a block..."
    local OUTPUT=$(tail -f "$NODE_PATH/logs/stdout.log" | grep -m 1 "proposer: PublicKey::Ed25519($PUBLIC_KEY_HEX)")
    log "node-$NODE_ID created a block!"
    log "$OUTPUT"
}


# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

STEP=0

main
