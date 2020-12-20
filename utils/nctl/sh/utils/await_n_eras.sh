#!/usr/bin/env bash

unset OFFSET
unset NET_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in        
        offset) OFFSET=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        *)
    esac
done

OFFSET=${OFFSET:-1}
NET_ID=${NET_ID:-1}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh

await_n_eras \
    $NET_ID \
    $(get_node_for_dispatch $NET_ID) \
    $OFFSET \
    true
