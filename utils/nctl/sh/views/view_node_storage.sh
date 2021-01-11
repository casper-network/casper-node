#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders status at specified node(s).
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
            do_render "$NODE_ID"
        done
        echo "------------------------------------------------------------------------------------------------------------------------------------"
    else
        do_render "$NODE_ID"
    fi
}

#######################################
# Displays to stdout current node storage stats.
# Globals:
#   _OS_LINUX - linux OS literal.
#   _OS_MACOSX - Mac OS literal.
# Arguments:
#   Node ordinal identifier.
#######################################
function do_render()
{
    local NODE_ID=${1}
    local OS_TYPE
    local PATH_TO_NODE_STORAGE
    
    OS_TYPE="$(get_os)"
    PATH_TO_NODE_STORAGE="$(get_path_to_node "$NODE_ID")/storage"

    log "node #$NODE_ID :: storage @ $PATH_TO_NODE_STORAGE"

    if [[ $OS_TYPE == "$_OS_LINUX*" ]]; then
        ll "$PATH_TO_NODE_STORAGE"
    elif [[ $OS_TYPE == "$_OS_MACOSX" ]]; then
        ls -lG "$PATH_TO_NODE_STORAGE"
    fi
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
        *)
    esac
done

main "${NODE_ID:-"all"}"
