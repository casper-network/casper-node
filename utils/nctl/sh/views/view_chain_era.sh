#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders chain era at specified node(s).
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#######################################
function main()
{
    local NODE_ID=${1}
    local TIMEOUT_SEC=${2}

    if [ "$NODE_ID" = "all" ]; then
        for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
        do
            log "chain era @ node-$NODE_ID = $(get_chain_era "$NODE_ID" "$TIMEOUT_SEC")"
        done
    else
        log "chain era @ node-$NODE_ID = $(get_chain_era "$NODE_ID" "$TIMEOUT_SEC")"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset NODE_ID
unset TIMEOUT_SEC

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        node) NODE_ID=${VALUE} ;;
        timeout) TIMEOUT_SEC=${VALUE} ;;
        *)
    esac
done

main "${NODE_ID:-"all"}" "${TIMEOUT_SEC:-"0"}"
