#!/usr/bin/env bash

unset NODE_ID
unset METRIC

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        metric) METRIC=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

METRIC=${METRIC:-"all"}
NODE_ID=${NODE_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh
source $NCTL/sh/views/funcs.sh

if [ $NODE_ID = "all" ]; then
    for NODE_ID in $(seq 1 $(get_count_of_nodes))
    do
        render_node_metrics $NODE_ID $METRIC
    done
else
    render_node_metrics $NODE_ID $METRIC
fi
