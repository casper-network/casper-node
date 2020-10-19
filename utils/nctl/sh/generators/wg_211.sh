#!/usr/bin/env bash
#
# Invokes auction.undelegate entry point.
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

# Unset to avoid parameter collisions.
unset amount
unset gas
unset net
unset node
unset payment
unset user
unset validator

# Destructure.
for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) amount=${VALUE} ;;
        gas) gas_price=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        payment) gas_payment=${VALUE} ;;
        user) user=${VALUE} ;;
        validator) validator=${VALUE} ;;
        *)
    esac
done

# Set defaults.
amount=${amount:-1000000}
gas_payment=${gas_payment:-200000}
gas_price=${gas_price:-10}
net=${net:-1}
node=${node:-1}
user=${user:-1}
validator=${validator:-1}

#######################################
# Main
#######################################

# Set vars.
contract_name="undelegate.wasm"
delegator_secret_key=$path_net/users/user-$user/secret_key.pem
node_address=$(get_node_address $net $node)
path_net=$NCTL/assets/net-$net
path_client=$path_net/bin/casper-client
path_contract=$path_net/bin/$contract_name
validator_public_key=`cat $path_net/nodes/node-$validator/keys/public_key_hex`

# Inform.
log "dispatching deploy -> "$contract_name
log "... network = $net"
log "... node = $node"
log "... node address = $node_address"
log "... contract = $path_contract"
log "... delegator id = $user"
log "... delegator secret key = $delegator_secret_key"
log "... amount = $amount"

# Dispatch deploy.
deploy_hash=$(
    $path_client put-deploy \
        --chain-name casper-net-$net \
        --gas-price $gas_price \
        --node-address $node_address \
        --payment-amount $gas_payment \
        --secret-key $delegator_secret_key \
        --session-arg "amount:u512='$amount'" \
        --session-arg "validator:public_key='$validator_public_key'" \
        --session-path $path_contract \
        --ttl "1day" | \
        python3 -c "import sys, json; print(json.load(sys.stdin)['deploy_hash'])"
    )

# Display deploy hash.
log "deploy dispatched:"
log "... deploy hash = $deploy_hash"
