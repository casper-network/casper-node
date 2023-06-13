#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh
source "$NCTL"/sh/assets/upgrade.sh
source "$NCTL"/sh/scenarios/common/itst.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

# Exit if any of the commands fail.
set -e

#######################################
# Test scenario against regression described in https://github.com/casper-network/casper-node/issues/3976
#
# 1. Run 1.2.0 network
# 2. Send a deploy without approvals (in 1.2.0 it'll be accepted, but executed with error)
# 3. Wait until deploy executes
# 4. Assert it executed with error
# 5. Upgrade network to local
# 6. Join node #6
# 7. Expect it syncs back to genesis, not getting stuck at the failed deploy
#######################################
function main() {
    log "------------------------------------------------------------"
    log "Regression #3976 begins"
    log "------------------------------------------------------------"

    # Prepare stage
    do_prepare_stage
    # Start the network
    do_start_network
    # Await for the completion of the genesis era
    do_await_genesis_era_to_complete
    # Send deploy with no approvals
    do_send_deploy_without_approvals
    # Make sure deploy got included and executed
    do_await_era_change "3"
    # Assert that deploy executed with expected error
    assert_have_expected_error_message
    # Stage the update
    do_upgrade
    # "Fix" the storage and config folders
    do_inject_sse_index
    do_inject_global_state_file
    # Wait for the upgrade to take place
    do_await_network_upgrade
    # Join 6th node
    join_6th_node
    # Assert the 6th node synced back to genesis
    await_node_historical_sync_to_genesis '6' "$SYNC_TIMEOUT_SEC"
}

function do_prepare_stage() {
    local PATH_TO_STAGE=${1}
    local STARTING_VERSION=${2}
    local INCREMENT
    local RC_VERSION

    log "... removing stray remotes and stages"
    rm -rf $(get_path_to_stages)
    rm -rf $(get_path_to_remotes)

    log "... setting remote 1.2.0"

    nctl-stage-set-remotes "1.2.0"

    log "... preparing settings"

    mkdir -p "$(get_path_to_stage '1')"

    cat <<EOF > "$(get_path_to_stage_settings 1)"
export NCTL_STAGE_SHORT_NAME="YOUR-SHORT-NAME"

export NCTL_STAGE_DESCRIPTION="YOUR-DESCRIPTION"

export NCTL_STAGE_TARGETS=(
    "1_2_0:remote"
    "$TEST_PROTOCOL_VERSION:remote"
    "$TEST_PROTOCOL_VERSION_2:local"
)
EOF

    log "... building stage from settings"

    nctl-stage-build-from-settings
}

function do_start_network() {
    nctl-assets-setup-from-stage stage=1
    nctl-start
}

function do_send_deploy_without_approvals() {
    log_step "sending deploy without approvals"

    local PATH_TO_OUTPUT
    local PATH_TO_OUTPUT_UNSIGNED
    local PATH_TO_OUTPUT_UNSIGNED_NO_APPROVALS
    local AMOUNT
    local DEPLOY_HASH

    AMOUNT=100
    PATH_TO_NET=$(get_path_to_net)
    PATH_TO_CLIENT=$(get_path_to_client)
    CHAIN_NAME=$(get_chain_name)
    CP1_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_FAUCET")
    PATH_TO_CONTRACT=$(get_path_to_contract "transfers/transfer_to_account_u512.wasm")
    CP2_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" 1)
    CP2_ACCOUNT_HASH=$(get_account_hash "$CP2_ACCOUNT_KEY")
    VALIDATOR_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_NODE" 1)
    NODE_ADDRESS=$(get_node_address_rpc)

    PATH_TO_OUTPUT="$PATH_TO_NET"/deploys/unapproved-wasm
    mkdir -p "$PATH_TO_OUTPUT"
    PATH_TO_OUTPUT_UNSIGNED="$PATH_TO_OUTPUT"/unapproved-wasm-unsigned.json
    PATH_TO_OUTPUT_UNSIGNED_NO_APPROVALS="$PATH_TO_OUTPUT_UNSIGNED".unapproved

    rm -f $PATH_TO_OUTPUT_UNSIGNED

    $PATH_TO_CLIENT make-deploy \
        --output "$PATH_TO_OUTPUT_UNSIGNED" \
        --chain-name "$CHAIN_NAME" \
        --payment-amount "$NCTL_DEFAULT_GAS_PAYMENT" \
        --ttl "5minutes" \
        --secret-key "$CP1_SECRET_KEY" \
        --session-arg "$(get_cl_arg_u512 'amount' "$AMOUNT")" \
        --session-arg "$(get_cl_arg_account_hash 'target' "$CP2_ACCOUNT_HASH")" \
        --session-path "$PATH_TO_CONTRACT" > \
        /dev/null 2>&1

    cat $PATH_TO_OUTPUT_UNSIGNED | jq 'del(.approvals[])' > $PATH_TO_OUTPUT_UNSIGNED_NO_APPROVALS
    rm -f $PATH_TO_OUTPUT_UNSIGNED

    NODE_ADDRESS=$(get_node_address_rpc)
    INVALID_DEPLOY_HASH=$(
        $PATH_TO_CLIENT send-deploy \
            --node-address "$NODE_ADDRESS" \
            --input "$PATH_TO_OUTPUT_UNSIGNED_NO_APPROVALS" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    )
}

function assert_have_expected_error_message() {
    log_step "asserting deploy executed with error"

    # using client fails with the following error:
    # 'response for rpc-id -8750037828368215590 info_get_deploy is json-rpc error: {"code":-32602,"message":"Invalid params"}'
    
    #OUTPUT=$($(get_path_to_client) get-deploy \
    #    --node-address "$(get_node_address_rpc)" \
    #    "$INVALID_DEPLOY_HASH")

    # to avoid hitting a potential incompatibility of the local client,
    # we use the RPC request directly

    local NODE_ADDRESS
    local REQUEST
    local OUTPUT
    local HAS_EXPECTED_ERROR_MESSAGE

    NODE_ADDRESS=$(get_node_address_rpc_for_curl)

    REQUEST="{\"id\": \"1\", \"jsonrpc\": \"2.0\", \"method\": \"info_get_deploy\", \"params\": { \"deploy_hash\": \"$INVALID_DEPLOY_HASH\" }}"
    OUTPUT=$(curl -d "$REQUEST" --header "Content-Type: application/json" $NODE_ADDRESS)
    HAS_EXPECTED_ERROR_MESSAGE=$(echo $OUTPUT | grep "Authorization failure: not authorized" | wc -l)

    if [ "$HAS_EXPECTED_ERROR_MESSAGE" -ne "1" ]; then
        log "ERROR: Deploy should fail with Authorization failure"
        exit 1
    fi
}

function do_upgrade() {
    ACTIVATE_ERA=$(($(get_chain_era)+2))
    log_step "scheduling the network upgrade to version ${TEST_PROTOCOL_VERSION_2} at era ${ACTIVATE_ERA}"
    nctl-assets-upgrade-from-stage stage="1" era="$ACTIVATE_ERA"
}

function do_inject_sse_index() {
    # sse_index file is required for the upgraded nodes to start
    # we inject it manually because we don't upgrade through all versions (in particular, we skip
    # the one upgrade that takes care of this file)
    log_step "injecting sse_index file"
    for NODE_ID in $(seq "1" "5")
    do
        PATH_NODE_STORAGE=$(get_path_to_node_storage $NODE_ID)
        echo -n 7 > $PATH_NODE_STORAGE/sse_index
    done
}

function do_inject_global_state_file() {
    # global_state.toml must be present in the config of the joining node, otherwise it
    # won't be able to talk to other nodes because the handshake process will
    # detect a chainspec hash mismatch
    log_step "injecting global state file"
    PATH_NODE_1_CONFIG=$(get_path_to_node_config "1")
    PATH_NODE_6_CONFIG=$(get_path_to_node_config "6")
    cp $PATH_NODE_1_CONFIG/1_5_0/global_state.toml $PATH_NODE_6_CONFIG/1_5_0/global_state.toml
}

function do_await_network_upgrade() {
    log_step "wait for the network to upgrade"
    local WAIT_TIME_SEC=0
    local WAIT_UNTIL=$((ACTIVATE_ERA + 1))
    while [ "$(get_chain_era)" != "$WAIT_UNTIL" ]; do
    if [ "$WAIT_TIME_SEC" = "$SYNC_TIMEOUT_SEC" ]; then
        log "ERROR: Failed to upgrade the network in ${SYNC_TIMEOUT_SEC} seconds"
            exit 1
        fi
        WAIT_TIME_SEC=$((WAIT_TIME_SEC + 1))
        sleep 1.0
    done
}

function join_6th_node() {
    log_step "joining 6th node"
    TRUSTED_HASH="$(get_chain_latest_block_hash)"
    do_node_start "6" "$TRUSTED_HASH"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SYNC_TIMEOUT_SEC
unset LFB_HASH
unset TEST_PROTOCOL_VERSION
unset TEST_PROTOCOL_VERSION_2
unset INVALID_DEPLOY_HASH
unset ACTIVATE_ERA

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
TEST_PROTOCOL_VERSION="1_2_0"
TEST_PROTOCOL_VERSION_2="1_5_0"

main
