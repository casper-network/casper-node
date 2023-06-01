#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration test that tries to simulate
# a single doppelganger situation.
#
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: itst11"
    log "------------------------------------------------------------"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Allow chain to progress
    do_await_era_change
    # 2. Verify all nodes are in sync
    parallel_check_network_sync 1 5
    # 3. Create the doppelganger
    create_doppelganger '5' '6'
    # 4. Get LFB Hash
    do_read_lfb_hash '5'
    # 5. Start doppelganger
    do_start_node "6" "$LFB_HASH"
    # 6. Look for one of the two nodes to report as faulty
    assert_equivication "5" "6"
    # 7. Run Health Checks
    # ... errors=ignore: can cause multiple errors non-deterministicly
    #                    see: sre issue 347.
    # ... doppels=ignore: doppelganger purposely created in this test
    # ... equivocators=ignore: doppelganger can cause an equivocation
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='ignore' \
            equivocators='ignore' \
            doppels='ignore' \
            crashes=0 \
            restarts=0 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario itst11 complete"
    log "------------------------------------------------------------"
}

function create_doppelganger() {
    local NODE_ID=${1}
    local DOPPEL_ID=${2}
    log_step "Copying keys from $NODE_ID into $DOPPEL_ID"
    cp -r "$(get_path_to_node $NODE_ID)/keys" "$(get_path_to_node $DOPPEL_ID)/"
}

function check_doppel() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    grep -q 'received vertex from a doppelganger' "$NODE_PATH/logs/stdout.log"
    return $?
}

function assert_equivication() {
    local NODE_ID=${1}
    local DOPPEL_ID=${2}
    log_step "Checking for a faulty node..."
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if ( check_faulty "$NODE_ID" ); then
            log "validator node-$NODE_ID found as faulty! [expected]"
            break
        elif ( check_faulty "$DOPPEL_ID" ); then
            log "doppelganger node-$DOPPEL_ID found as faulty! [expected]"
            break
        elif ( check_doppel "$NODE_ID" ); then
            log "node-$NODE_ID received vertex from a doppelganger! [expected]"
            break
        elif ( check_doppel "$DOPPEL_ID" ); then
            log "node-$DOPPEL_ID received vertex from a doppelganger! [expected]"
            break
        fi

        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Failed to confirm a faulty validator"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1
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

SYNC_TIMEOUT_SEC=${SYNC_TIMEOUT_SEC:-"300"}

main
