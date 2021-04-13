#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh

#######################################
# Renders user on-chain account details.
# Arguments:
#   Node ordinal identifier.
#######################################
function main()
{
    local USER_ID=${1}

    if [ "$USER_ID" = "all" ]; then
        for USER_ID in $(seq 1 "$(get_count_of_users)")
        do
            echo "------------------------------------------------------------------------------------------------------------------------------------"
            render "$USER_ID"
        done
    else
        render "$USER_ID"
    fi
}

#######################################
# Validator account details renderer.
# Globals:
#   NCTL_ACCOUNT_TYPE_USER - node account type literal.
# Arguments:
#   Node ordinal identifier.
#######################################
function render()
{
    local USER_ID=${1}

    PATH_TO_ACCOUNT_SKEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_USER" "$USER_ID")
    ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" "$USER_ID")
    ACCOUNT_HASH=$(get_account_hash "$ACCOUNT_KEY")
    STATE_ROOT_HASH=$(get_state_root_hash)
    PURSE_UREF=$(get_main_purse_uref "$ACCOUNT_KEY" "$STATE_ROOT_HASH")
    ACCOUNT_BALANCE=$(get_account_balance "$PURSE_UREF" "$STATE_ROOT_HASH")

    log "user #$USER_ID a/c secret key    : $PATH_TO_ACCOUNT_SKEY"
    log "user #$USER_ID a/c key           : $ACCOUNT_KEY"
    log "user #$USER_ID a/c hash          : $ACCOUNT_HASH"
    log "user #$USER_ID a/c purse         : $PURSE_UREF"
    log "user #$USER_ID a/c purse balance : $ACCOUNT_BALANCE"
    log "user #$USER_ID on-chain account  : see below"
    render_account "$NCTL_ACCOUNT_TYPE_USER" "$USER_ID"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset USER_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        user) USER_ID=${VALUE} ;;
        *)
    esac
done

main "${USER_ID:-"all"}"
