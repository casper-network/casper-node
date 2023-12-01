#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

# Trapping exit so we can remove the temp file used in ci run
trap clean_up EXIT

function clean_up() {
    local EXIT_CODE=$?
    local STDOUT
    local STDERR
    local TMP_CLIENT_DIR

    TMP_CLIENT_DIR='/tmp/client'

    if [ -d "$TMP_CLIENT_DIR" ]; then
        log "Removing client tmp dir..."
        rm -rf "$TMP_CLIENT_DIR"
    fi

    # Removes DEPLOY_LOG for ITST06/07
    if [ -f "$DEPLOY_LOG" ]; then
        log "Removing transfer tmp file..."
        rm -f "$DEPLOY_LOG"
    fi

    log "Test exited with exit code $EXIT_CODE"

    # On failure dump the logs
    if [ "$EXIT_CODE" == '1' ]; then
        log "Dumping logs..."
        nctl-assets-dump
        # If CI, upload to s3
        if [ ! -z "$AWS_ACCESS_KEY_ID" ] && [ ! -z "$AWS_SECRET_ACCESS_KEY" ] && [ "$DRONE" == 'true' ]; then
            log "Uploading dump to s3..."
            pushd $(get_path_to_net_dump)
            tar -cvzf "${DRONE_BUILD_NUMBER}"_nctl_dump.tar.gz * > /dev/null 2>&1
            aws s3 cp ./"${DRONE_BUILD_NUMBER}"_nctl_dump.tar.gz s3://nctl.casperlabs.io/nightly-logs/ > /dev/null 2>&1
            log "Download the dump file: curl -O https://s3.us-east-2.amazonaws.com/nctl.casperlabs.io/nightly-logs/${DRONE_BUILD_NUMBER}_nctl_dump.tar.gz"
            log "\nextra log lines to push\ndownload instructions above\nserver license expired banner\n"
            popd
        fi
    fi

    exit $EXIT_CODE
}

function log_step() {
    local COMMENT=${1}
    log "------------------------------------------------------------"
    log "STEP $STEP: $COMMENT"
    STEP=$((STEP + 1))
}

function do_wait_until_era() {
    local WAIT_FOR_ERA=${1}
    local LOG_STEP=${2:-'true'}
    local TIMEOUT=${3:-'120'}

    if [ "$LOG_STEP" = "true" ]; then
        log_step "waiting until era $WAIT_FOR_ERA: timeout=$TIMEOUT"
    fi

    while [ "$(get_chain_era)" -lt "$WAIT_FOR_ERA" ]; do
        sleep 1.0
        TIMEOUT=$((TIMEOUT-1))
        if [ "$TIMEOUT" = '0' ]; then
            log "ERROR: Timed out before reaching era $WAIT_FOR_ERA"
            exit 1
        else
            log "... waiting for era $WAIT_FOR_ERA to complete: timeout=$TIMEOUT"
        fi
    done
}

function do_read_lfb_hash() {
    local NODE_ID=${1}
    LFB_HASH=$(render_last_finalized_block_hash "$NODE_ID" | cut -f2 -d= | cut -f2 -d ' ')
    echo "$LFB_HASH"
}

function do_start_node() {
    local NODE_ID=${1}
    local LFB_HASH=${2}
    log_step "starting node-$NODE_ID. Syncing from hash=${LFB_HASH}"

    # Deleting the pid due to crash that gets logged
    if [ -f "$(get_path_to_node_storage $NODE_ID)/initializer.pid" ]; then
        log "... pid file detected, removing (<= 1.3)"
        rm -f "$(get_path_to_node_storage $NODE_ID)/initializer.pid"
    elif [ -f "$(get_path_to_node_storage $NODE_ID)/casper-net-1/initializer.pid" ]; then
        log "... pid file detected, removing (1.4+)"
        rm -f "$(get_path_to_node_storage $NODE_ID)/casper-net-1/initializer.pid"
    fi

    do_node_start "$NODE_ID" "$LFB_HASH"
    sleep 1
    if [ "$(do_node_status ${NODE_ID} | awk '{ print $2 }')" != "RUNNING" ]; then
        log "ERROR: node-${NODE_ID} is not running"
        nctl-status
        exit 1
    else
        log "node-${NODE_ID} is running, started in era: $(check_current_era)"
    fi
}

function do_stop_node() {
    local NODE_ID=${1}
    log_step "stopping node-$NODE_ID."
    do_node_stop "$NODE_ID"
    sleep 1
    if [ "$(do_node_status ${NODE_ID} | awk '{ print $2 }')" = "RUNNING" ]; then
        log "ERROR: node-${NODE_ID} is still running"
    exit 1
    else
        log "node-${NODE_ID} was shutdown in era: $(check_current_era)"
    fi
}

function get_reactor_state() {
    local NODE_ID=${1}
    local OUTPUT
    local REACTOR_STATE

    OUTPUT=$(nctl-view-node-status node=$NODE_ID)
    REACTOR_STATE=$(echo "$OUTPUT" | tail -n +2 | jq -r '.reactor_state')

    echo "$REACTOR_STATE"
}

function write_lfb_of_node_to_pipe() {
    local LOG=${3:-'true'}

    HASH=$(get_chain_latest_block_hash $1)
    if [ "$LOG" = 'true' ]; then
        log "writing $HASH for node $1 to pipe"
    fi
    echo "$HASH" >$2
}

function parallel_check_network_sync() {
    local FIRST_NODE=${1:-1}
    local LAST_NODE=${2:-10}
    local SYNC_TIMEOUT_SEC=${3:-"$SYNC_TIMEOUT_SEC"}
    local LOG=${4:-'true'}

    local PIPE=/tmp/casper_lfb_test_pipe
    local ALL_HASHES=""
    local EXPECTED
    local ATTEMPTS=0

    rm -f $PIPE
    if [[ ! -p $PIPE ]]; then
        mkfifo $PIPE
    fi

    if [ "$LOG" = 'true' ]; then
        log_step "checking nodes' $FIRST_NODE to $LAST_NODE LFBs are in sync"
    fi

    while [ "$ATTEMPTS" -le "$SYNC_TIMEOUT_SEC" ]; do
        ALL_HASHES=""
        EXPECTED=$((LAST_NODE-FIRST_NODE+1))
        for IDX in $(seq "$FIRST_NODE" "$LAST_NODE")
        do
            write_lfb_of_node_to_pipe $IDX $PIPE $LOG &
        done

        while true
        do
            if read LINE;
            then
                if [ "$LOG" = 'true' ]; then
                    log "read $LINE from pipe"
                fi
                ALL_HASHES="$ALL_HASHES $LINE"
                EXPECTED=$((EXPECTED - 1))
                if [ "$LOG" = 'true' ]; then
                    log "expected $EXPECTED"
                fi
                if [ "$EXPECTED" -eq "0" ]; then
                    break
                fi
            fi
        done < $PIPE
        wait

        if [ "$LOG" = 'true' ]; then
            log "I have all hashes: $ALL_HASHES"
        fi

        UNIQUE_HASH_COUNT=$(echo $ALL_HASHES | tr " " "\n" | sort | uniq | wc -l)
        if [ "$LOG" = 'true' ]; then
            log "unique hash count: $UNIQUE_HASH_COUNT"
        fi

        if [ "$UNIQUE_HASH_COUNT" -eq 1 ]; then
            log "nodes $FIRST_NODE to $LAST_NODE in sync, proceeding..."
            nctl-view-chain-height
            rm -f $PIPE
            break
        fi
        ATTEMPTS=$((ATTEMPTS + 1))
        if [ "$ATTEMPTS" -lt "$SYNC_TIMEOUT_SEC" ]; then
            sleep 1
            if [ "$LOG" = 'true' ]; then
                log "attempt $ATTEMPTS out of $SYNC_TIMEOUT_SEC..."
            fi
        else
            log "ERROR: Failed to confirm network sync"
            nctl-status
            nctl-view-chain-height
            rm -f $PIPE
            exit 1
        fi
    done
}

function do_await_era_change() {
    # allow chain height to grow
    local ERA_COUNT=${1:-"1"}
    log_step "awaiting $ERA_COUNT eras…"
    nctl-await-n-eras offset="$ERA_COUNT" sleep_interval='5.0'
}

function do_await_era_change_with_timeout() {
    # allow chain height to grow
    local ERA_COUNT=${1:-"1"}
    local TIME_OUT=${2:-''}
    log_step "awaiting $ERA_COUNT eras…"
    nctl-await-n-eras offset="$ERA_COUNT" sleep_interval='5.0' timeout="$TIME_OUT"
}

function check_current_era {
    local NODE_ID=${1:-$(get_node_for_dispatch)}
    local ERA="null"
    while true; do
        ERA="$(get_chain_era $NODE_ID | awk '{print $NF}')"
        if [ "$ERA" = "null" ] || [ "$ERA" = "N/A" ]; then
            sleep 1
        else
            break
        fi
    done
    echo "$ERA"
}

function check_faulty() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    grep -q 'this validator is faulty' "$NODE_PATH/logs/stdout.log"
    return $?
}

# Captures the public key in hex of a node minus the '01' prefix.
# This is because the logs don't include the '01' prefix when
# searching in them for the public key in hex. See itst14 for
# use case.
function get_node_public_key_hex() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(cat "$NODE_PATH"/keys/public_key_hex)
    echo "${PUBLIC_KEY_HEX:2}"
}

# Same as get_node_public_key_hex but includes the '01' prefix.
# When parsing the state with jq, the '01' prefix is included.
# See itst13 for use case.
function get_node_public_key_hex_extended() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node $NODE_ID)
    local PUBLIC_KEY_HEX=$(cat "$NODE_PATH"/keys/public_key_hex)
    echo "$PUBLIC_KEY_HEX"
}

function do_await_n_blocks() {
    local BLOCK_COUNT=${1:-1}
    log_step "Waiting $BLOCK_COUNT blocks..."
    nctl-await-n-blocks offset="$BLOCK_COUNT"
}

# Gets the header of the switch block of the given era.
function get_switch_block() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local BLOCK_HASH=${3}
    local ERA=${4}

    if [ -z "$BLOCK_HASH" ]; then
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID"))
    else
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID") -b "$BLOCK_HASH")
    fi

    if [ "$WALKBACK" -gt 0 ]; then
        BLOCK=$(echo "$JSON_OUT" | jq '.result.block')
        local ERA_END="$(echo "$BLOCK" | jq '.header.era_end')"
        local ERA_ID="$(echo "$BLOCK" | jq '.header.era_id')"
        if [ "$ERA_END" = "null" ] || { [ -n "$ERA" ] && [ "$ERA_ID" != "$ERA" ]; }; then
            PARENT=$(echo "$BLOCK" | jq -r '.header.parent_hash')
            WALKBACK=$((WALKBACK - 1))
            get_switch_block "$NODE_ID" "$WALKBACK" "$PARENT" "$ERA"
        else
            echo "$BLOCK"
        fi
    else
        echo "null"
    fi
}

function get_switch_block_equivocators() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local BLOCK_HASH=${3}

    local BLOCK=$(get_switch_block "$NODE_ID" "$WALKBACK" "$BLOCK_HASH")

    if [ "$BLOCK" != "null" ]; then
        log "equivocators: $(echo "$BLOCK" | jq '.header.era_end.era_report.equivocators')"
    else
        log "Error: Switch block not found within walkback!"
        exit 1
    fi
}

# Function is used to walk back the blocks to check if a transfer
# is included under transfer_hashes. If the transfer is not found
# within the walkback, it will error out.
function verify_transfer_inclusion() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local TRANSFER=${3}
    local BLOCK_HASH=${4}
    local JSON_OUT
    local PARENT
    local BLOCK_HEADER
    local BLOCK_TRANSFER_HASHES

    if [ -z "$BLOCK_HASH" ]; then
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID"))
    else
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID") -b "$BLOCK_HASH")
    fi

    if [ "$WALKBACK" -gt 0 ]; then
        BLOCK_HEADER=$(echo "$JSON_OUT" | jq '.result.block.header')
        BLOCK_TRANSFER_HASHES=$(echo "$JSON_OUT" | jq -r '.result.block.body.transfer_hashes[]')
        if grep -q "${TRANSFER}" <<< "$BLOCK_TRANSFER_HASHES"; then
            log "Transfer: $TRANSFER found in block!"
        else
            PARENT=$(echo "$BLOCK_HEADER" | jq -r '.parent_hash')
            WALKBACK=$((WALKBACK - 1))
            log "$WALKBACK: Walking back to block: $PARENT"
            verify_transfer_inclusion "$NODE_ID" "$WALKBACK" "$TRANSFER" "$PARENT"
        fi
    else
        log "Error: Transfer $TRANSFER not found within walkback!"
        exit 1
    fi
}

function verify_wasm_inclusion() {
    local NODE_ID=${1}
    # Number of blocks to walkback before erroring out
    local WALKBACK=${2}
    local DEPLOY_HASH=${3}
    local BLOCK_HASH=${4}
    local JSON_OUT
    local PARENT
    local BLOCK_HEADER
    local BLOCK_DEPLOY_HASHES

    if [ -z "$BLOCK_HASH" ]; then
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID"))
    else
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$NODE_ID") -b "$BLOCK_HASH")
    fi

    if [ "$WALKBACK" -gt 0 ]; then
        BLOCK_HEADER=$(echo "$JSON_OUT" | jq '.result.block.header')
        BLOCK_DEPLOY_HASHES=$(echo "$JSON_OUT" | jq -r '.result.block.body.deploy_hashes[]')
        if grep -q "${DEPLOY_HASH}" <<< "$BLOCK_DEPLOY_HASHES"; then
            log "DEPLOY: $DEPLOY_HASH found in block!"
        else
            PARENT=$(echo "$BLOCK_HEADER" | jq -r '.parent_hash')
            WALKBACK=$((WALKBACK - 1))
            log "$WALKBACK: Walking back to block: $PARENT"
            verify_wasm_inclusion "$NODE_ID" "$WALKBACK" "$DEPLOY_HASH" "$PARENT"
        fi
    else
        log "Error: Deploy $DEPLOY_HASH not found within walkback!"
        exit 1
    fi
}

function get_running_node_count {
    local RUNNING_COUNT=$(nctl-status | grep 'RUNNING' | wc -l)
    echo "$RUNNING_COUNT"
}

# Check that a certain node has produced blocks.
function assert_node_proposed() {
    local NODE_ID=${1}
    local NODE_PATH=$(get_path_to_node "$NODE_ID")
    local PUBLIC_KEY_HEX=$(get_node_public_key_hex_extended "$NODE_ID")
    local TIMEOUT=${2:-300}
    local OUTPUT
    local PROPOSER

    log_step "Waiting for a node-$NODE_ID to produce a block"
    log "...node-$NODE_ID public key hex: $PUBLIC_KEY_HEX"

    while true; do
        OUTPUT=$($(get_path_to_client) get-block --node-address "$(get_node_address_rpc)")
        PROPOSER=$(echo "$OUTPUT" | jq -r '.result.block.body.proposer')

        if [ "$PROPOSER" == "$PUBLIC_KEY_HEX" ]; then
            log "Node-$NODE_ID created a block!"
            log "$OUTPUT"
            log "Proposer: $PROPOSER"
            break;
        else
            sleep 1
            TIMEOUT=$((TIMEOUT-1))
            if [ "$TIMEOUT" = '0' ]; then
                log "ERROR: Timed out before node created a block"
                exit 1
            else
                log "...waiting for node to propose: timeout=$TIMEOUT"
            fi
        fi
    done
}

function assert_no_proposal_walkback() {
    local NODE_ID=${1}
    local WALKBACK_HASH=${2}
    local NODE_PATH=$(get_path_to_node "$NODE_ID")
    local PUBLIC_KEY_HEX=$(get_node_public_key_hex_extended "$NODE_ID")
    local COMPARE_NODE=$(get_node_for_dispatch)
    local CHECK_HASH=$(do_read_lfb_hash "$COMPARE_NODE")
    local JSON_OUT
    local PROPOSER

    # normalize hashes
    WALKBACK_HASH=$(echo $WALKBACK_HASH |  tr '[:upper:]' '[:lower:]')
    CHECK_HASH=$(echo $CHECK_HASH | tr '[:upper:]' '[:lower:]')

    log_step "Checking proposers: Walking back to hash $WALKBACK_HASH..."

    while [ "$CHECK_HASH" != "$WALKBACK_HASH" ]; do
        JSON_OUT=$($(get_path_to_client) get-block --node-address $(get_node_address_rpc "$COMPARE_NODE") -b "$CHECK_HASH")
        PROPOSER=$(echo "$JSON_OUT" | jq -r '.result.block.body.proposer')
        if [ "$PROPOSER" = "$PUBLIC_KEY_HEX" ]; then
            log "ERROR: Node proposal found!"
            log "BLOCK HASH $CHECK_HASH: PROPOSER=$PROPOSER, NODE_KEY_HEX=$PUBLIC_KEY_HEX"
            log "$JSON_OUT"
            exit 1
        else
            log "BLOCK HASH $CHECK_HASH: PROPOSER=$PROPOSER, NODE_KEY_HEX=$PUBLIC_KEY_HEX"
            unset CHECK_HASH
            CHECK_HASH=$(echo $JSON_OUT | jq -r '.result.block.header.parent_hash')
            # normalize
            CHECK_HASH=$(echo $CHECK_HASH | tr '[:upper:]' '[:lower:]')
            log "Checking next hash: $CHECK_HASH"
        fi
    done
    log "Walkback Completed!"
    log "CHECK_HASH: $CHECK_HASH = WALKBACK_HASH: $WALKBACK_HASH"
    log "Node $NODE_ID didn't propose! [expected]"
}

# Checks logs for nodes 1-10 for equivocators
function assert_no_equivocators_logs() {
    local LOGS
    log_step "Looking for equivocators in logs..."
    for i in $(seq 1 $(get_count_of_nodes)); do
        # true is because grep exits 1 on no match
        LOGS=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null | grep -w 'validator equivocated') || true
        log "... checking node-$i's log"
        if [ ! -z "$LOGS" ]; then
            log "$(echo $LOGS)"
            log "ERROR: Equivocator(s) found!"
            exit 1
        else
            log "... No Equivocator(s) found! [expected]"
        fi
    done
    log "Finished Equivocator(s) search successfully!"
}

function do_submit_auction_bids()
{
    local NODE_ID=${1}
    log_step "submitting POS auction bids:"
    log "----- ----- ----- ----- ----- -----"
    BID_AMOUNT="1000000000000000000000000000000"
    BID_DELEGATION_RATE=${1}

    source "$NCTL"/sh/contracts-auction/do_bid.sh \
            node="$NODE_ID" \
            amount="$BID_AMOUNT" \
            rate="$BID_DELEGATION_RATE" \
            quiet="FALSE"

    log "node-$NODE_ID auction bid submitted -> $BID_AMOUNT CSPR"

    log "awaiting 10 seconds for auction bid deploys to finalise"
    sleep 10.0
}

function assert_same_era() {
    local ERA=${1}
    local NODE_ID=${2}
    log_step "Checking if within same era..."
    if [ "$ERA" = "$(check_current_era $NODE_ID)" ]; then
        log "Still within the era! [expected]"
        log "... ERA = $ERA : CURRENT_ERA = $(check_current_era $NODE_ID)"
    else
        log "Error: Era progressed! Exiting..."
        log "... ERA = $ERA : CURRENT_ERA = $(check_current_era $NODE_ID)"
        exit 1
    fi
}

# Checks that the current era + 1 contains a nodes
# public key hex
function is_trusted_validator() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex_extended "$NODE_ID")
    local ERA=$(check_current_era)
    # Plus 1 to avoid query issue if era switches mid run
    local ERA_PLUS_1=$(expr $ERA + 1)
    # note: tonumber is a must here to prevent jq from being too smart.
    # The jq arg 'era' is set to $ERA_PLUS_1. The query looks to find that
    # the validator is removed from era_validators list. We grep for
    # the public_key_hex to see if the validator is still listed and return
    # the exit code to check success.
    nctl-view-chain-auction-info | jq --arg era "$ERA_PLUS_1" '.auction_state.era_validators[] | select(.era_id == ($era | tonumber))' | grep -q "$HEX"
    return $?
}

function assert_eviction() {
    local NODE_ID=${1}
    local SYNC_TIMEOUT_SEC=${2:-"300"}
    local WAIT_TIME_SEC='0'

    log_step "Checking for evicted node-$NODE_ID..."
    while [ "$WAIT_TIME_SEC" != "$SYNC_TIMEOUT_SEC" ]; do
        if ( ! is_trusted_validator "$NODE_ID" ); then
            log "validator node-$NODE_ID was ejected! [expected]"
            break
        fi

        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))

        if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
            log "ERROR: Time out. Failed to confirm node-$NODE_ID as evicted validator in $SYNC_TIMEOUT_SEC seconds."
            exit 1
        fi
        sleep 1
    done
}

function assert_new_bonded_validator() {
    local NODE_ID=${1}
    local HEX=$(get_node_public_key_hex "$NODE_ID")
    if ! $(nctl-view-chain-auction-info | grep -q "$HEX"); then
      echo "Could not find key in bids"
      exit 1
    fi
}

function delegate_to() {
    local NODE_ID=${1}
    local ACCOUNT_ID=${2}
    local AMOUNT=${3}

    log_step "Delegating $AMOUNT from account-$ACCOUNT_ID to validator-$NODE_ID"

    source "$NCTL/sh/contracts-auction/do_delegate.sh" \
        amount="$AMOUNT" \
        delegator="$ACCOUNT_ID" \
        validator="$NODE_ID"
}

function assert_chain_stalled() {
    # Fucntion checks that the two remaining node's LFB checked
    # n-seconds apart doesnt progress
    log_step "ensuring chain stalled"
    local SLEEP_TIME=${1}
    # Sleep 5 seconds to allow for final message propagation.
    sleep 5
    local LFB_1_PRE=$(do_read_lfb_hash 1)
    local LFB_2_PRE=$(do_read_lfb_hash 2)
    log "Sleeping ${SLEEP_TIME}s..."
    sleep $SLEEP_TIME
    local LFB_1_POST=$(do_read_lfb_hash 1)
    local LFB_2_POST=$(do_read_lfb_hash 2)

    if [ "$LFB_1_PRE" != "$LFB_1_POST" ] && [ "$LFB_2_PRE" != "$LFB_2_POST" ]; then
       log "Error: Chain progressed."
       exit 1
    else
        STALLED_LFB=$LFB_1_POST
        log "node-1 LFB: $LFB_1_PRE = $LFB_1_POST"
        log "node-2 LFB: $LFB_2_PRE = $LFB_2_POST"
        log "Stall successfully detected, continuing..."
    fi
}