#!/usr/bin/env bash
#
# Invokes auction.add-bid entry point.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier (int).
#   Node ordinal identifier (int).
#   User ordinal identifier (int).
#   Bid amount (motes).
#   Delegation rate (float).

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset amount
unset payment
unset gas
unset delegation_rate
unset net
unset node
unset user

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) amount=${VALUE} ;;
        gas) gas=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        payment) payment=${VALUE} ;;
        rate) delegation_rate=${VALUE} ;;
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
amount=${amount:-$NCTL_DEFAULT_AUCTION_BID_AMOUNT}
delegation_rate=${delegation_rate:-125}
payment=${payment:-$NCTL_DEFAULT_GAS_PAYMENT}
gas=${gas:-$NCTL_DEFAULT_GAS_PRICE}
net=${net:-1}
node=${node:-1}
user=${user:-1}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

# Set deploy params.
bidder_secret_key=$(get_path_to_secret_key $net $NCTL_ACCOUNT_TYPE_USER $user)
bidder_public_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $user)
node_address=$(get_node_address_rpc $net $node)
path_contract=$(get_path_to_contract $net "add_bid.wasm")

# Inform.
log "dispatching deploy -> add_bid.wasm"
log "... network = $net"
log "... node = $node"
log "... node address = $node_address"
log "... contract = $path_contract"
log "... bidder id = $user"
log "... bidder secret key = $bidder_secret_key"
log "... bid amount = $amount"
log "... bid delegation rate = $delegation_rate"

# Dispatch deploy.
deploy_hash=$(
    $(get_path_to_client $net) put-deploy \
        --chain-name casper-net-$net \
        --gas-price $gas \
        --node-address $node_address \
        --payment-amount $payment \
        --secret-key $bidder_secret_key \
        --session-arg="public_key:public_key='$bidder_public_key'" \
        --session-arg "amount:u512='$amount'" \
        --session-arg "delegation_rate:u64='$delegation_rate'" \
        --session-path $path_contract \
        --ttl "1day" \
        | jq '.result.deploy_hash' \
        | sed -e 's/^"//' -e 's/"$//'
    )

# Display deploy hash.
log "deploy dispatched:"
log "... deploy hash = $deploy_hash"
