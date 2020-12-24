#!/usr/bin/env bash

source $NCTL/sh/utils/main.sh

#######################################
# Renders chain height at specified node(s).
# Arguments:
#   Node ordinal identifier.
#######################################
function main()
{
    local NODE_ID=${1}

    if [ $NODE_ID = "all" ]; then
        for NODE_ID in $(seq 1 $(get_count_of_nodes))
        do
            log "chain height @ node-$NODE_ID = $(get_chain_height $NODE_ID)"
        done
    else
        log "chain height @ node-$NODE_ID = $(get_chain_height $NODE_ID)"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

main ${NODE_ID:-"all"}
