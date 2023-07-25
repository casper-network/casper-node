#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that tries to simulate
# wasm being sent to a node that falls over mid-stream.
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: itst07"
    log "------------------------------------------------------------"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Verify network is creating blocks
    do_await_n_blocks '5'
    # 2. Verify network is in sync
    parallel_check_network_sync 1 5
    # 3. Background transfers so we can stop the node mid-stream
    do_background_wasm_transfers '5'
    # 4. Stop node being sent transfers
    do_stop_node '5'
    # 5. Wait for the background job to complete
    log_step "Waiting for background job to complete"
    wait
    # 6. Get LFB and restart stopped node
    do_read_lfb_hash '1'
    do_start_node '5' "$LFB_HASH"
    # 7. Verify network is in sync
    parallel_check_network_sync 1 5
    # 8. Give the transfers a chance to be included
    do_await_n_blocks '30'
    # 9. Walkback and verify transfers were included in blocks
    check_wasm_inclusion '1' '1000'
    # 10. Run Health Checks
    # ... restarts=1: due to node being stopped and started
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=1 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario itst07 complete"
    log "------------------------------------------------------------"
}

# Transfers sent in background so we can mimic a node dying mid-stream.
function do_background_wasm_transfers() {
    local NODE_ID=${1}
    log_step "initiated background wasm transfers"
    (bash "$NCTL"/sh/contracts-transfers/do_dispatch_wasm.sh \
        transfers=100 \
        amount=1 \
        node="$NODE_ID") > "$DEPLOY_LOG" 2>&1 &
    sleep 1
}

# Loops lines the hashes in the temp file. Check if all transfers we received a hash
# back from the client are included in a block.
function check_wasm_inclusion() {
    local NODE_ID=${1}
    local WALKBACK=${2}
    local TRANSFER_HASHES=$(cat "$DEPLOY_LOG" | awk -F'::' '{print $4}' | sed 's/^[ \t]*//' | sed '/^[[:space:]]*$/d' | sed '/FAILED to send/d')
    local TRANSFER_COUNT=$(echo "$TRANSFER_HASHES" | wc -l)
    local HASH
    log_step "Checking wasm inclusion..."
    for (( i=1; i<="$TRANSFER_COUNT"; i++ )); do
        HASH=$(echo "$TRANSFER_HASHES" | sed -n "$i"p)
        if [ -z "$HASH" ]; then
            log "Error: No Hash found!"
            exit 1
        fi
        log "Starting walkback for Transfer $i: $HASH"
        verify_wasm_inclusion "$NODE_ID" "$WALKBACK" "$HASH"
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
        deploy_log) DEPLOY_LOG=${VALUE} ;;
        *) ;;
    esac
done

SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}
DEPLOY_LOG=${DEPLOY_LOG:-"/tmp/itst07.out"}

main "$NODE_ID"
