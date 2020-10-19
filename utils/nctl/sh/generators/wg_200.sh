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

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# TODO: accept purse uref

# Unset to avoid parameter collisions.
unset amount
unset gas_payment
unset gas_price
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
        gas) gas_price=${VALUE} ;;
        rate) delegation_rate=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        payment) gas_payment=${VALUE} ;;
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
amount=${amount:-1000000}
delegation_rate=${delegation_rate:-125}
gas_payment=${gas_payment:-200000}
gas_price=${gas_price:-10}
net=${net:-1}
node=${node:-1}
user=${user:-1}

#######################################
# Main
#######################################

# Set vars.
bidder_secret_key=$path_net/users/user-$user/secret_key.pem
contract_name="add_bid.wasm"
node_address=$(get_node_address $net $node)
path_net=$NCTL/assets/net-$net
path_client=$path_net/bin/casper-client
path_contract=$path_net/bin/$contract_name

# Inform.
log "dispatching deploy -> "$contract_name
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
    $path_client put-deploy \
        --chain-name casper-net-$net \
        --gas-price $gas_price \
        --node-address $node_address \
        --payment-amount $gas_payment \
        --secret-key $bidder_secret_key \
        --session-arg "amount:u512='$amount'" \
        --session-arg "delegation_rate:u64='$delegation_rate'" \
        --session-path $path_contract \
        --ttl "1day" | \
        python3 -c "import sys, json; print(json.load(sys.stdin)['deploy_hash'])"
    )

# Display deploy hash.
log "deploy dispatched:"
log "... deploy hash = $deploy_hash"
