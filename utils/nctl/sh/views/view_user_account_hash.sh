#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh

#######################################
# Renders user account hash.
# Globals:
#   NCTL_ACCOUNT_TYPE_USER - user account type literal.
# Arguments:
#   User ordinal identifier.
#######################################
function main()
{
    local USER_ID=${1}

    if [ "$USER_ID" = "all" ]; then
        for USER_ID in $(seq 1 "$(get_count_of_users)")
        do
            render_account_hash "$NCTL_ACCOUNT_TYPE_USER" "$USER_ID"
        done
    else
        render_account_hash "$NCTL_ACCOUNT_TYPE_USER" "$USER_ID"
    fi
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

