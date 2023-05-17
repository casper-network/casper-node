#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

function main() {
    local PRE_SWAP_HASH
    local POST_SWAP_HASH
    local SWITCHBLOCK_HASH

    log "------------------------------------------------------------"
    log "Starting Scenario: swap_validator_set"
    log "------------------------------------------------------------"

    # 0. Wait for network to start up
    do_await_genesis_era_to_complete

    # 1. Allow the chain to progress
    do_await_era_change 1
    PRE_SWAP_HASH=$(do_read_lfb_hash 1)

    # 2. Verify all nodes are in sync
    parallel_check_network_sync 1 5

    # 3. Send some wasm to all running nodes
    log_step "sending wasm trandfers to validators"
    for i in $(seq 1 5); do
        send_wasm_transfers "$i"
    done

    # 4. Bid in nodes 6-10
    log_step "bidding node's 6 thru 10 for takeover"
    for i in $(seq 6 10); do
        source "$NCTL"/sh/contracts-auction/do_bid.sh \
                node="$i" \
                amount="1000" \
                rate="0"
    done

    # 5-9. Start nodes 6-10
    for i in $(seq 6 10); do
        do_start_node "$i" "$PRE_SWAP_HASH"
    done

    # 10. Wait auction_delay + 2
    log_step "waiting until era 8 where swap should take place"
    nctl-await-until-era-n era='8' log='true'

    # Since this walks back to first found switch block, keep this immediately after era 8 starts
    SWICHBLOCK_HASH=$(get_switch_block '1' '100' | jq -r '.hash' )

    # 11. Assert nodes 6-10 in auction info
    log_step "checking for new validators"
    for i in $(seq 6 10); do
        assert_new_bonded_validator "$i"
        log "node-$i found in auction info"
    done
    nctl-view-chain-auction-info

    # 12-16. Assert nodes 1-5 are evicted
    for i in $(seq 1 5); do
        assert_eviction "$i"
    done

    # 17. Reuse nodes 1, 4, 5
    reuse_original_validators_as_new_nodes

    # 18. Start node 1 with pre validator swap block hash
    do_node_start '1' "$PRE_SWAP_HASH"

    # 19. Start node 4 with post validator swap block hash
    POST_SWAP_HASH=$(do_read_lfb_hash 6)
    do_node_start '4' "$POST_SWAP_HASH"

    # 20. Start node 5 with switch block hash
    do_node_start '5' "$SWICHBLOCK_HASH"

    # 21. Lets some time pass
    nctl-await-n-eras offset='3' sleep_interval='10.0' timeout='300' node='6'

    # 22. Check network is in sync
    parallel_check_network_sync 1 10

    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='0' \
            equivocators=0 \
            doppels=0 \
            crashes=0 \
            restarts=0 \
            ejections=0

    log "------------------------------------------------------------"
    log "Scenario swap_validator_set complete"
    log "------------------------------------------------------------"
}


# Hack to reuse original 3 validators as completely new nodes.
# Uses existing unused user keys.
# Note: Tom will revisit this later.

function reuse_original_validators_as_new_nodes() {
    local DUMP_PATH
    local NODE_LOG_PATH
    local NODE_STORAGE_PATH
    local NODE_KEYS_PATH
    local USER_KEYS_PATH

    log_step "Converting nodes 1, 4, 5 to 'new' joiners"

    # place to move old node stuff
    DUMP_PATH="/tmp/swap_validator_set"

    # cleanup if it exists already
    if [ -d "$DUMP_PATH" ]; then
        rm -rf "$DUMP_PATH"
    fi

    # make sure it exists
    mkdir -p "$DUMP_PATH"

    # loop thru the 3 nodes
    for i in 1 4 5; do
        # stop the three nodes that will become make shift
        # joining nodes 11, 12, 13
        log "...stopping node-$i"
        do_node_stop "$i"

        # mv logs in case needed for failed runs
        log "...moving logs to $DUMP_PATH"
        NODE_LOG_PATH=$(get_path_to_node_logs "$i")
        echo "cp $NODE_LOG_PATH/stdout.log $DUMP_PATH/$i.stdout.log"
        mv "$NODE_LOG_PATH/stdout.log" "$DUMP_PATH/$i.stdout.log"
        mv "$NODE_LOG_PATH/stderr.log" "$DUMP_PATH/$i.stderr.log"
        # delete storage
        log "...moving previous state"
        NODE_STORAGE_PATH=$(get_path_to_node_storage "$i")
        mv "$NODE_STORAGE_PATH/casper-net-1" "$DUMP_PATH/$i-storage"

        # move in "new" validator keys
        log "... switching in user keys as validator keys"
        NODE_KEYS_PATH=$(get_path_to_node_keys "$i")
        USER_KEYS_PATH=$(get_path_to_user "$i")
        cp "$USER_KEYS_PATH/public_key.pem" "$NODE_KEYS_PATH/public_key.pem"
        cp "$USER_KEYS_PATH/public_key_hex" "$NODE_KEYS_PATH/public_key_hex"
        cp "$USER_KEYS_PATH/secret_key.pem" "$NODE_KEYS_PATH/secret_key.pem"
    done
}

function send_wasm_transfers() {
    local NODE_ID=${1}
    log "sending wasm transfers to node-$NODE_ID"
    source "$NCTL"/sh/contracts-transfers/do_dispatch_wasm.sh \
        transfers=100 \
        amount=2500000000 \
        node="$NODE_ID" \
        verbose=false
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

main "$NODE_ID"
