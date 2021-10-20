#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that tries to simulate
# transfers being sent to a node that falls over 
# mid-stream.
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: itst06"
    log "------------------------------------------------------------"

    # 0. Verify network is creating blocks
    do_await_n_blocks '5'
    # 1. Verify network is in sync
    check_network_sync
    # 2. Background transfers so we can stop the node mid-stream
    do_background_wasmless_transfers '5'
    # 3. Stop node being sent transfers
    do_stop_node '5'
    # 4. Wait for the background job to complete
    log_step "Waiting for background job to complete"
    wait
    # 5. Get LFB and restart stopped node
    do_read_lfb_hash '1'
    do_start_node '5' "$LFB_HASH"
    # 6. Verify network is in sync
    check_network_sync
    # 7. Give the tranfers a chance to be included
    do_await_n_blocks '30'
    # 8. Walkback and verify transfers were included in blocks
    check_transfer_inclusion '1' '1000'
    # 10. Run Health Checks
    # ... errors=ignore: ticket sre issue 71
    # ... restarts=1: due to node being stopped and started
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='ignore' \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=1 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario itst06 complete"
    log "------------------------------------------------------------"
}

# Transfers sent in background so we can mimic a node dying mid-stream.
function do_background_wasmless_transfers() {
    local NODE_ID=${1}
    log_step "initiated background wasmless transfers"
    (bash "$NCTL"/sh/contracts-transfers/do_dispatch_native.sh \
        transfers=100 \
        amount=2500000000 \
        node="$NODE_ID") > "$DEPLOY_LOG" 2>&1 &
    sleep 1
}

# Loops lines the hashes in the temp file. Check if all transfers we recieved a hash
# back from the client are included in a block.
function check_transfer_inclusion() {
    local NODE_ID=${1}
    local WALKBACK=${2}
    local TRANSFER_HASHES=$(cat "$DEPLOY_LOG" | awk -F'::' '{print $4}' | sed 's/^[ \t]*//' | sed '/^[[:space:]]*$/d')
    local TRANSFER_COUNT=$(echo "$TRANSFER_HASHES" | wc -l)
    local HASH
    log_step "Checking transfer inclusion..."
    for (( i='1'; i<="$TRANSFER_COUNT"; i++ )); do
        HASH=$(echo "$TRANSFER_HASHES" | sed -n "$i"p)
        if [ -z "$HASH" ]; then
            log "Error: No Hash found!"
            exit 1
        fi
        log "Starting walkback for Transfer $i: $HASH"
        verify_transfer_inclusion "$NODE_ID" "$WALKBACK" "$HASH"
        log ""
    done
}


# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset DEPLOY_LOG
unset LFB_HASH
STEP=0

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        timeout) SYNC_TIMEOUT_SEC=${VALUE} ;;
        deploy_log) DEPLOY_LOG=$(VALUE} ;;
        *) ;;
    esac
done

SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}
DEPLOY_LOG=${DEPLOY_LOG:-"/tmp/itst06.out"}

main "$NODE_ID"
