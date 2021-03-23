#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh


#######################################
# Clean resources of specified nodeset.
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
            do_clean "$NODE_ID"
        done
        echo "------------------------------------------------------------------------------------------------------------------------------------"
    else
        do_clean "$NODE_ID"
    fi
}

#######################################
# Clean resources of specified node(s).
# Arguments:
#   Node ordinal identifier.
#######################################
function do_clean()
{
    local NODE_ID=${1}

    # Stop node.
    if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
        source "$NCTL"/sh/node/stop.sh node="$NODE_ID"
    fi

    # Remove logs.
    rm "$(get_path_to_node_logs "$NODE_ID")"/*.log > /dev/null 2>&1

    # Remove state.
    rm "$(get_path_to_node_storage "$NODE_ID")"/*.lmdb* > /dev/null 2>&1
    rm "$(get_path_to_node_storage "$NODE_ID")"-consensus/*.dat > /dev/null 2>&1

    log "cleaned node-$NODE_ID"
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
