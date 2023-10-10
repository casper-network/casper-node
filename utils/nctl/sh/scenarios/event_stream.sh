#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

TMP_EVENTS_FILE=tmp_events_main.txt

#######################################
# Runs an integration tests that verifies if events are routed via correct SSE endpoints.
# Prior to https://github.com/casper-network/casper-node/issues/4314 there were 3 different
# endpoints, serving different events. The 3 events that this test verifies used to be
# routed as follows:
# - `BlockAdded`        via /events/main
# - `DeployAccepted`    via /events/deploys
# - `FinalitySignature` via /events/sigs
#
# Currently, all events should be emitted via `/events`. We check only for the above three
# explicitly mentioned events as more detailed tests are located in the `tests` module
# of the `event_stream_server`.
#
# Additionally, this test verifies that `/events/main`, `/events/deploys` and `/events/sigs`
# are no longer accessible.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Event stream test begins"
    log "------------------------------------------------------------"

    do_await_genesis_era_to_complete
    do_check_endpoint_does_not_exist 'deploys'
    do_check_endpoint_does_not_exist 'sigs'
    do_check_endpoint_does_not_exist 'main'
    do_attach_to_event_stream
    do_send_wasm_deploys
    await_n_blocks 1 false
    do_check_event_emitted 'BlockAdded'
    do_check_event_emitted 'DeployAccepted'
    do_check_event_emitted 'FinalitySignature'

    log "------------------------------------------------------------"
    log "Event stream test complete"
    log "------------------------------------------------------------"

    # Clean-up temporary event file on successful run
    rm $TMP_EVENTS_FILE
}

function do_check_event_emitted() {
    local EVENT_NAME=${1}

    log_step "checking '$EVENT_NAME' was emitted"

    if ! grep -q $EVENT_NAME $TMP_EVENTS_FILE
    then
        log "ERROR: '$EVENT_NAME' event not found"
        exit 1
    fi

}

function do_attach_to_event_stream() {
    log_step "attaching to event stream"
    curl -o $TMP_EVENTS_FILE -N -s localhost:18101/events &
}

function do_check_endpoint_does_not_exist() {
    local RESPONSE
    local EVENT_PATH=${1}

    log_step "checking endpoint '$EVENT_PATH' does not exist"

    RESPONSE=$(curl -s localhost:18101/events/{$EVENT_PATH})
    if [ "$RESPONSE" != "invalid path: expected '/events'" ]; then
        exit 1
    fi
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
