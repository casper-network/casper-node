#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

unset NODE_ID
unset WITHDRAWAL_AMOUNT

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        amount) WITHDRAWAL_AMOUNT=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

NODE_ID=${NODE_ID:-1}
WITHDRAWAL_AMOUNT=${WITHDRAWAL_AMOUNT:-$(get_node_staking_weight "$NODE_ID")}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Submit auction withdrawal.
log "dispatching auction withdrawal deploy"
source "$NCTL"/sh/contracts-auction/do_bid_withdraw.sh \
    node="$NODE_ID" \
    amount="$WITHDRAWAL_AMOUNT"

# If node is up then await & stop.
if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
    log "awaiting 4 eras"
    await_n_eras 4 true
    log "node-$NODE_ID: stopping node ... "
    do_node_stop "$NODE_ID"
fi
