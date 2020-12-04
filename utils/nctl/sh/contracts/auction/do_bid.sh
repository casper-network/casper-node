#!/usr/bin/env bash
#
# Invokes auction.add-bid entry point.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier (optional).
#   Node ordinal identifier (optional).
#   User ordinal identifier (optional).
#   Bid amount (optional).
#   Delegation rate (optional).
#   Gas price (optional).
#   Gas payment (optional).

#######################################
# Imports
#######################################

# Import utils.
source $NCTL/sh/utils.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset AMOUNT
unset DELEGATION_RATE
unset GAS
unset NET_ID
unset NODE_ID
unset PAYMENT
unset USER_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        gas) GAS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        payment) PAYMENT=${VALUE} ;;
        rate) DELEGATION_RATE=${VALUE} ;;
        user) USER_ID=${VALUE} ;;
        *)
    esac
done

# Set defaults.
AMOUNT=${AMOUNT:-$NCTL_DEFAULT_AUCTION_BID_AMOUNT}
DELEGATION_RATE=${DELEGATION_RATE:-125}
GAS=${GAS:-$NCTL_DEFAULT_GAS_PRICE}
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}
PAYMENT=${PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
USER_ID=${USER_ID:-1}

#######################################
# Main
#######################################

# Set deploy params.
BIDDER_PUBLIC_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $USER_ID)
BIDDER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $USER_ID)
NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
PATH_TO_CONTRACT=$(get_path_to_contract $NET_ID "add_bid.wasm")

# Inform.
log "dispatching deploy -> add_bid.wasm"
log "... network = $NET_ID"
log "... node = $NODE_ID"
log "... node address = $NODE_ADDRESS"
log "... contract = $PATH_TO_CONTRACT"
log "... bidder id = $USER_ID"
log "... bidder secret key = $BIDDER_SECRET_KEY"
log "... bid amount = $amount"
log "... bid delegation rate = $DELEGATION_RATE"

# Dispatch deploy.
DEPLOY_HASH=$(
    $(get_path_to_client $NET_ID) put-deploy \
        --chain-name $(get_chain_name $NET_ID) \
        --gas-price $GAS \
        --node-address $NODE_ADDRESS \
        --payment-amount $PAYMENT \
        --secret-key $BIDDER_SECRET_KEY \
        --session-arg="public_key:public_key='$BIDDER_PUBLIC_KEY'" \
        --session-arg "amount:u512='$AMOUNT'" \
        --session-arg "delegation_rate:u64='$DELEGATION_RATE'" \
        --session-path $PATH_TO_CONTRACT \
        --ttl "1day" \
        | jq '.result.deploy_hash' \
        | sed -e 's/^"//' -e 's/"$//'
    )

# Display deploy hash.
log "deploy dispatched:"
log "... deploy hash = $DEPLOY_HASH"
