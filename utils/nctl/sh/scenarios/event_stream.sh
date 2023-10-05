#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that verifies if events are routed via correct SSE endpoints.
# - `BlockAdded`        via /events/main
# - `DeployAccepted`    via /events/deploys
# - `FinalitySignature` via /events/sigs
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Event stream test begins"
    log "------------------------------------------------------------"

    do_await_genesis_era_to_complete

    ################################################
    curl -o events_main.txt -N -s localhost:18101/events/main &
    log "Attached to event stream output (main)"

    curl -o events_deploys.txt -N -s localhost:18101/events/deploys &
    log "Attached to event stream output (deploys)"

    curl -o events_sigs.txt -N -s localhost:18101/events/sigs &
    log "Attached to event stream output (sigs)"

    log "Awaiting one block"
    await_n_blocks 1 false

    cat events_main.txt
    if grep -q BlockAdded events_main.txt
    then
        log "Found"
    else
        log "ERROR: BlockAdded Not found"
        exit 1
    fi

    ################################################


    do_send_wasm_deploys

    cat events_deploys.txt
    if grep -q DeployAccepted events_deploys.txt
    then
        log "Found"
    else
        log "ERROR: DeployAccepted Not found"
        exit 1
    fi


    ################################################


    cat events_sigs.txt
    if grep -q FinalitySignature events_sigs.txt
    then
        log "Found"
    else
        log "ERROR: FinalitySignature Not found"
        exit 1
    fi

    log "------------------------------------------------------------"
    log "Event stream test complete"
    log "------------------------------------------------------------"
}

function do_send_wasm_deploys() {
    # NOTE: Maybe make these arguments to the test?
    local BATCH_COUNT=1
    local BATCH_SIZE=1
    local TRANSFER_AMOUNT=2500000000
    log_step "sending Wasm deploys"
    # prepare wasm batch
    prepare_wasm_batch "$TRANSFER_AMOUNT" "$BATCH_COUNT" "$BATCH_SIZE"
    # dispatch wasm batches
    for BATCH_ID in $(seq 1 $BATCH_COUNT); do
        dispatch_wasm_batch "$BATCH_ID"
    done
}

function prepare_wasm_batch() {
    local AMOUNT=${1}
    local BATCH_COUNT=${2}
    local BATCH_SIZE=${3}

    source "$NCTL"/sh/contracts-transfers/do_prepare_wasm_batch.sh amount="$AMOUNT" \
        count="$BATCH_COUNT" \
        size="$BATCH_SIZE"
}

function dispatch_wasm_batch() {
    local BATCH_ID=${1:-1}
    local INTERVAL=${2:-0.01}
    local NODE_ID=${3:-"random"}

    source "$NCTL"/sh/contracts-transfers/do_dispatch_wasm_batch.sh batch="$BATCH_ID" \
        interval="$INTERVAL" \
        node="$NODE_ID"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
