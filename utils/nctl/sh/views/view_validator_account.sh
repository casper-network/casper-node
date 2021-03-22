#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh

#######################################
# Renders node on-chain account details.
# Arguments:
#   Node ordinal identifier.
#######################################
function main()
{
    local NODE_ID=${1}

    if [ "$NODE_ID" = "all" ]; then
        for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
        do
            echo "------------------------------------------------------------------------------------------------------------------------------------"
            render "$NODE_ID"
        done
    else
        render "$NODE_ID"
    fi
}

#######################################
# Validator account details renderer.
# Globals:
#   NCTL_ACCOUNT_TYPE_NODE - node account type literal.
# Arguments:
#   Node ordinal identifier.
#######################################
function render()
{
    local NODE_ID=${1}

    PATH_TO_ACCOUNT_SKEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_NODE" "$NODE_ID")
    ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$NODE_ID")
    ACCOUNT_HASH=$(get_account_hash "$ACCOUNT_KEY")
    STATE_ROOT_HASH=$(get_state_root_hash)
    PURSE_UREF=$(get_main_purse_uref "$ACCOUNT_KEY" "$STATE_ROOT_HASH")
    ACCOUNT_BALANCE=$(get_account_balance "$PURSE_UREF" "$STATE_ROOT_HASH")

    log "validator #$NODE_ID a/c secret key    : $PATH_TO_ACCOUNT_SKEY"
    log "validator #$NODE_ID a/c key           : $ACCOUNT_KEY"
    log "validator #$NODE_ID a/c hash          : $ACCOUNT_HASH"
    log "validator #$NODE_ID a/c purse         : $PURSE_UREF"
    log "validator #$NODE_ID a/c purse balance : $ACCOUNT_BALANCE"
    log "validator #$NODE_ID on-chain account  : see below"
    render_account "$NCTL_ACCOUNT_TYPE_NODE" "$NODE_ID"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        node) NODE_ID=${VALUE} ;;
        validator) NODE_ID=${VALUE} ;;
        *)
    esac
done

main "${NODE_ID:-"all"}"
