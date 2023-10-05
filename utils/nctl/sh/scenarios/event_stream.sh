#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that verifies if events are routed via correct SSE endpoints.
# Prior to https://github.com/casper-network/casper-node/issues/4314 there were 3 different
# endpoints, serving different evvents. The 3 events that this test verifies used to be
# routed as follows:
# - `BlockAdded`        via /events/main
# - `DeployAccepted`    via /events/deploys
# - `FinalitySignature` via /events/sigs
#
# Currently, all events should be emitted via `/events/main`. We check only for the above three
# explicitly mentioned events as more detailed tests are located in the `tests` module
# of the `event_stream_server`.
#
# Additionally, this test verifies that `/events/deploys` and `/events/sigs` are no longer accessible.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Event stream test begins"
    log "------------------------------------------------------------"

    do_await_genesis_era_to_complete

    ################################################
    NO_DEPLOYS_PATH=$(curl -s localhost:18101/events/deploys)
    if [ "$NO_DEPLOYS_PATH" = "invalid path: expected '/events/main'" ]; then
        echo "The strings are equal."
    else
        echo "The strings are different."
        exit 1
    fi

    NO_SIGS_PATH=$(curl -s localhost:18101/events/sigs)
    if [ "$NO_SIGS_PATH" = "invalid path: expected '/events/main'" ]; then
        echo "The strings are equal."
    else
        echo "The strings are different."
        exit 1
    fi

    ################################################
    curl -o events_main.txt -N -s localhost:18101/events/main &
    log "Attached to event stream output (main)"

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

    cat events_main.txt
    if grep -q DeployAccepted events_main.txt
    then
        log "Found"
    else
        log "ERROR: DeployAccepted Not found"
        exit 1
    fi


    ################################################


    cat events_main.txt
    if grep -q FinalitySignature events_main.txt
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
