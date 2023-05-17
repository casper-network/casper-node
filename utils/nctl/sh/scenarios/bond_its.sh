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
    parallel_check_network_sync 1 5
    # 3. Submit bid for node 6
    do_submit_auction_bids "6"
    do_read_lfb_hash "5"
    do_start_node "6" "$LFB_HASH"
    # 4. wait auction_delay + 1
    do_await_era_change "4"
    # 5. Assert that the validator is bonded in.
    assert_new_bonded_validator "6"
    log "The new node has bonded in."
    # 6. Assert that the new bonded validator is producing blocks.
    assert_node_proposed "6" "180"
    # 7. Unbond node
    unbond_validator "6"
    # 8. Wait auction delay + 1
    do_await_era_change "4"
    # 9. Assert unbonded
    assert_eviction "6"
    # Gather Block Hash after evicted node for walkback later
    local AFTER_EVICTION_HASH=$(do_read_lfb_hash '1')
    # 10. Wait 3 eras to see if the node ends up proposing
    do_await_era_change "3"
    # Node 6 must be synced to genesis in case it's selected for walkback
    await_node_historical_sync_to_genesis '6' '300'
    # 11. Assert node didn't propose since being unbonded
    assert_no_proposal_walkback '6' "$AFTER_EVICTION_HASH"
    # 12. Run Health Checks
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=0 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario bonding complete"
    log "------------------------------------------------------------"

}

function assert_new_bonded_validator() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex "$NODE_ID")
    if ! $(nctl-view-chain-auction-info | grep -q "$HEX"); then
      echo "Could not find key in bids"
      exit 1
    fi
}

function unbond_validator() {
    local NODE_ID=${1}
    local WITHDRAWAL_AMOUNT=${2:-"1000000000000000000000000000000"}
    log_step "submitting auction withdrawals"

    source "$NCTL"/sh/contracts-auction/do_bid_withdraw.sh \
            node="$NODE_ID" \
            amount="$WITHDRAWAL_AMOUNT" \
            quiet="FALSE"

    log "node-$NODE_ID auction bid withdrawn -> $WITHDRAWAL_AMOUNT CSPR"
    log "awaiting 10 seconds for auction withdrawl deploy to finalize"
    sleep 10.0
}
# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

STEP=0

main
