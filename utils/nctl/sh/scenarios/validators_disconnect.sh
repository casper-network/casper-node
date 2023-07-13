#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: Half of validators disconnecting"
    log "------------------------------------------------------------"

    # 0. Start the rest of the validator nodes
    log_step "Starting nodes 6-10"
    nctl-start node=6
    nctl-start node=7
    nctl-start node=8
    nctl-start node=9
    nctl-start node=10
    # 1. Wait for the genesis era to complete
    do_await_genesis_era_to_complete
    # 2. Verify all nodes are in sync
    parallel_check_network_sync 1 10
    # 3. Stop half of the validators
    log_step "Stopping nodes 6-10"
    nctl-stop node=6
    nctl-stop node=7
    nctl-stop node=8
    nctl-stop node=9
    nctl-stop node=10
    # 4. Assert that the chain actually stalled
    assert_chain_stalled "30"
    # 5. Wait for a period longer than the dead air interval
    sleep_and_display_reactor_state '260.0'
    # 6. Start 2 previously stopped validator nodes.
    #    Now the network should have 7/10 active and start progressing.
    log_step "Starting nodes 6, 7"
    nctl-start node=6
    nctl-start node=7
    # 7. Check if the network is progressing
    do_await_era_change_with_timeout 1 "500"
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=2 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario half of validators disconnecting complete"
    log "------------------------------------------------------------"
}

function sleep_and_display_reactor_state() {
    local TIME_OUT=${1:-'360.0'}
    local NODE_ID=${2:-"1"}
    local SLEEP_INTERVAL='10.0'

    log_step "Waiting $TIME_OUT seconds to pass…"
    while true
    do
        local REACTOR_STATUS=$(nctl-view-node-status node=1 | tail -n +2)
        local REACTOR_STATE=$(echo $REACTOR_STATUS | jq '.reactor_state')
        local HIGHEST_BLOCK=$(echo $REACTOR_STATUS | jq '.available_block_range.high' )
        LOG_OUTPUT="reactor state for node $NODE_ID = $REACTOR_STATE; highest block = $HIGHEST_BLOCK :: sleeping $SLEEP_INTERVAL seconds"

        if [ "$EMIT_LOG" = true ] && [ ! -z "$TIME_OUT" ]; then
            log "$LOG_OUTPUT :: sleep time = $TIME_OUT seconds"
        elif [ "$EMIT_LOG" = true ]; then
            log "$LOG_OUTPUT"
        fi

        sleep "$SLEEP_INTERVAL"

        if [ ! -z "$TIME_OUT" ]; then
            # Using jq since its required by NCTL anyway to do this floating point arith
            # ... done to maintain backwards compatibility
            TIME_OUT=$(jq -n "$TIME_OUT-$SLEEP_INTERVAL")

            if [ "$TIME_OUT" -le "0" ]; then
                log "Finished waiting"
                break
            fi
        else
            log "Finished waiting"
            break
        fi
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

STEP=0

main
