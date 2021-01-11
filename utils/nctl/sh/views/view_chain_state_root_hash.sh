#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/views/utils.sh

#######################################
# Renders chain state root hash at specified node(s).
# Arguments:
#   Node ordinal identifier.
#   Block hash.
#######################################
function main()
{
    local NODE_ID=${1}
    local BLOCK_HASH=${2}

    if [ "$NODE_ID" = "all" ]; then
        for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
        do
            render_chain_state_root_hash "$NODE_ID" "$BLOCK_HASH"
        done
    else
        render_chain_state_root_hash "$NODE_ID" "$BLOCK_HASH"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset BLOCK_HASH
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        block) BLOCK_HASH=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

main "${NODE_ID:-"all"}" "${BLOCK_HASH:-""}"
