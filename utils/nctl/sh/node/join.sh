#!/usr/bin/env bash

source "$NCTL"/sh/utils/infra.sh
source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

unset NODE_ID
unset BID_AMOUNT
unset BID_DELEGATION_RATE

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        amount) BID_AMOUNT=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        rate) BID_DELEGATION_RATE=${VALUE} ;;
        *)
    esac
done

NODE_ID=${NODE_ID:-6}
BID_AMOUNT=${BID_AMOUNT:-$(get_node_staking_weight "$NODE_ID")}
BID_DELEGATION_RATE=${BID_DELEGATION_RATE:-2}


# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

log "awaiting genesis era to complete"
do_await_genesis_era_to_complete false

# Submit auction bid.
log "dispatching auction bid deploy"
source "$NCTL/sh/contracts-auction/do_bid.sh" \
    node="$NODE_ID" \
    amount="$BID_AMOUNT" \
    rate="$BID_DELEGATION_RATE"

# Await until time to start node.
log "awaiting 3 eras + 1 block"
await_n_eras 3 true
await_n_blocks 1 false

# Start/Restart node (with trusted hash).
if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
    log "restarting node :: node-$NODE_ID"
    do_node_restart "$NODE_ID" "$(get_chain_latest_block_hash)"
else
    log "starting node :: node-$NODE_ID"
    do_node_start "$NODE_ID" "$(get_chain_latest_block_hash)"
fi
