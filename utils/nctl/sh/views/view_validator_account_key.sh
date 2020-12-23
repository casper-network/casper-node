#!/usr/bin/env bash

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

NODE_ID=${NODE_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh
source $NCTL/sh/views/funcs.sh

if [ $NODE_ID = "all" ]; then
    for NODE_ID in $(seq 1 $(get_count_of_nodes))
    do
        render_account_key $NCTL_ACCOUNT_TYPE_NODE $NODE_ID
    done
else
    render_account_key $NCTL_ACCOUNT_TYPE_NODE $NODE_ID
fi
