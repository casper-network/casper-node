#!/usr/bin/env bash
#
# Invokes auction.withdraw-bid entry point.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier (int).
#   Node ordinal identifier (int).
#   User ordinal identifier (int).
#   Withdrawal amount (motes).

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset amount
unset gas
unset payment
unset net
unset node
unset user

# Destructure.
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
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
amount=${amount:$NCTL_DEFAULT_AUCTION_BID_AMOUNT}
payment=${payment:-$NCTL_DEFAULT_GAS_PAYMENT}
gas=${gas:-$NCTL_DEFAULT_GAS_PRICE}
net=${net:-1}
node=${node:-1}
user=${user:-1}

#######################################
# Main
#######################################

# Set deploy params.
user_secret_key=$(get_path_to_secret_key $net $NCTL_ACCOUNT_TYPE_USER $user)
user_purse_uref="TODO"
user_public_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $user)
node_address=$(get_node_address_rpc $net $node)
path_contract=$(get_path_to_contract $net "withdraw_bid.wasm")

# Inform.
log "dispatching deploy -> withdraw_bid.wasm"
log "... network = $net"
log "... node = $node"
log "... node address = $node_address"
log "... contract = $path_contract"
log "... user id = $user"
log "... user public key = $user_public_key"
log "... user secret key = $user_secret_key"
log "... user purse uref = $user_purse_uref"
log "... withdrawal amount = $amount"

# Dispatch deploy.
deploy_hash=$(
    $(get_path_to_client $net) put-deploy \
        --chain-name casper-net-$net \
        --gas-price $gas \
        --node-address $node_address \
        --payment-amount $payment \
        --secret-key $user_secret_key \
        --session-arg="public_key:public_key='$user_public_key'" \
        --session-arg "amount:u512='$amount'" \
        --session-arg "unbond_purse:uref-='$user_purse_uref'" \
        --session-path $path_contract \
        --ttl "1day" \
        | jq '.result.deploy_hash' \
        | sed -e 's/^"//' -e 's/"$//'
    )

# Display deploy hash.
log "deploy dispatched:"
log "... deploy hash = $deploy_hash"
