#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/auction/funcs.sh

unset BIDDER_ID
unset BID_AMOUNT
unset BID_DELEGATION_RATE

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) BID_AMOUNT=${VALUE} ;;
        bidder) BIDDER_ID=${VALUE} ;;
        rate) BID_DELEGATION_RATE=${VALUE} ;;
        *)
    esac
done

BIDDER_ID=${BIDDER_ID:-6}
BID_AMOUNT=${BID_AMOUNT:-$(($NCTL_VALIDATOR_BASE_WEIGHT * $BIDDER_ID))}
BID_DELEGATION_RATE=${BID_DELEGATION_RATE:-125}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/contracts/auction/do_bid.sh \
    bidder=$BIDDER_ID \
    amount=$BID_AMOUNT \
    rate=$BID_DELEGATION_RATE

await_n_eras 0 true

source $NCTL/sh/node/start.sh \
    node=$BIDDER_ID 
