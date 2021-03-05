#!/usr/bin/env bash

SCENARIO="itst06"

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh
source "$NCTL"/sh/utils/infra.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration test that simulates
# shutting down a validator node during a
# wasmless deployment.
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: $SCENARIO"
    log "------------------------------------------------------------"

    log "Node to be stopped: $NODE_ID"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Allow chain to progress
    do_await_era_change
    # 2. Send wasmless deploys to random node (backgrounded).
    do_background_wasmless_transfers "$NODE_ID"
    # 3. Stop node
    do_stop_node "$NODE_ID"
    # 4. Allow chain to progress
    do_await_era_change
    # 5. Get another random running node to compare
    do_get_another_node
    # 6. Read the latest block hash of this second node.
    do_read_lfb_hash "$COMPARE_NODE_ID"
    # 7. Restart node from LFB
    do_start_node
    # 8. Ensure pending deploys drain.
    do_verify_deploys_drain
    # 9. Check sync of restarted node
    do_await_full_synchronization

    log "------------------------------------------------------------"
    log "Scenario $SCENARIO complete"
    log "------------------------------------------------------------"
}

function do_await_full_synchronization() {
    local WAIT_TIME_SEC=0
    log_step "awaiting full synchronization of node=${NODE_ID}…"
    log "DEBUG: $(do_node_status ${NODE_ID})"
    log "DEBUG: $(do_node_status ${COMPARE_NODE_ID})"
    while [ "$(do_read_lfb_hash "$NODE_ID")" != "$(do_read_lfb_hash "$COMPARE_NODE_ID")" ]; do
	log "NODE HASH: $(do_read_lfb_hash "$NODE_ID")"
	log "COMPARE HASH: $(do_read_lfb_hash "$COMPARE_NODE_ID")"
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to synchronize in ${SYNC_TIMEOUT_SEC} seconds"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
    # Wait 2 more era and then test chainheight.
    # This way we can verify that the node is up-to-date with the protocol state
    # after transitioning to an active validator and the chain progressed.
    log "NODE HASH: $(do_read_lfb_hash "$NODE_ID")"
    log "COMPARE HASH: $(do_read_lfb_hash "$COMPARE_NODE_ID")"
    do_await_era_change
    log_step "verifying full synchronization of node=${NODE_ID}…"
    while [ "$(get_chain_height "$NODE_ID")" != "$(get_chain_height "$COMPARE_NODE_ID")" ]; do
        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to keep up with the protocol state"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
}

function do_get_another_node() {
    COMPARE_NODE_ID=$(get_node_for_dispatch)
    log_step "comparison node: $COMPARE_NODE_ID"
}

function do_background_wasmless_transfers() {
    nohup bash "$NCTL"/sh/contracts-transfers/do_dispatch_native.sh \
        transfers=100 \
        amount=2500000000 \
        node="$NODE_ID" \
        >/dev/null 2>&1 \
        &

    sleep 1
    log_step "initiated background wasmless transfers"
}

function num_pending() {
    ENDPOINT="$(get_node_address_rest "$NODE_ID")"/metrics
    NUM_PENDING=""
    while [ -z "$NUM_PENDING" ]
    do
        NUM_PENDING="$(curl -s --location --request GET "$ENDPOINT" \
            | grep "pending_deploy" \
            | tail -n 1 \
            | sed -r 's/.*pending_deploy ([0-9]+)/\1/g')"
        sleep 1
    done
    # echo "Num pending: $NUM_PENDING"
}

function do_verify_deploys_drain() {
    log_step "waiting for pending deploys to drain"
    num_pending

    while [[ "$NUM_PENDING" > "0" ]]
    do
        PREV_NUM_PENDING=$NUM_PENDING
        num_pending
        if [[ $PREV_NUM_PENDING < $NUM_PENDING ]]
        then
            exit 1
        fi

        sleep 3
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
STEP=0

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        timeout) SYNC_TIMEOUT_SEC=${VALUE} ;;
        *) ;;
    esac
done

NODE_ID=$(get_node_for_dispatch)
SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}

main "$NODE_ID"
