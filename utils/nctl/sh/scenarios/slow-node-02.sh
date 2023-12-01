#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

#######################################
# Runs an integration tests that tries to simulate
# a slow validator in a network with uneven stakes
# and check if it did slow down.
# Note: needs the accounts.toml.override file to
# simulate the uneven stakes.
# Arguments:
#   `timeout=XXX` timeout (in seconds) when syncing.
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Starting Scenario: slow-node-02"
    log "------------------------------------------------------------"

    # 0. Wait for network start up
    do_await_genesis_era_to_complete
    # 1. Verify network is creating blocks
    do_await_n_blocks "5"
    # 2. Verify network is in sync
    parallel_check_network_sync 1 5
    INIT_ROUND_LEN=$(get_node_round_len 1)
    # 3. Slow down node
    slow_down_node_network 1
    # 4. Wait 2 eras
    do_await_era_change 2
    # Dump consensus data from node 2 for era 2 for debugging
    dump_consensus 2 2
    # 5. Verify all nodes are in sync
    parallel_check_network_sync 1 5
    # 6. Check that round length of node 1 is different
    NEW_ROUND_LEN=$(get_node_round_len 1)
    assert_round_len_changed "$INIT_ROUND_LEN" "$NEW_ROUND_LEN"
    # 7. Run Health Checks
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors=0 \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=0 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario slow-node-02 complete"
    log "------------------------------------------------------------"
}

function assert_round_len_changed() {
    local LEN1=${1}
    local LEN2=${2}
    if [ "$LEN1" = "$LEN2" ]; then
        log "Round length didn't change!"
        exit 1
    else
        log "Round length changed from $LEN1 to $LEN2 - OK"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
unset PUBLIC_KEY_HEX
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
