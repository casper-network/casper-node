#!/usr/bin/env bash
#
# Approves a ERC-20 token transfer from an account.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier (optional).
#   Node ordinal identifier (optional).
#   Gas price (optional).
#   Gas payment (optional).
#   Amount to be transferred (optional).
#   Node ordinal identifier (optional).

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

# Destructure named args.
for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        # ... standard args
        gas) gas=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        payment) payment=${VALUE} ;;
        # ... custom args
        amount) amount=${VALUE} ;;
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
amount=${amount:-1000000000}
gas=${gas:-$NCTL_DEFAULT_GAS_PRICE}
payment=${payment:-1000000000}
net=${net:-1}
node=${node:-1}
user=${user:-1}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import net vars.
source $(get_path_to_net_vars $net)

# Set amount - target contract uses U256 therefore need to factor to U512.
amount=$(($amount * 2))

# Set contract owner secret key.
contract_owner_secret_key=$(get_path_to_secret_key $net $NCTL_ACCOUNT_TYPE_FAUCET)

# Set user account key.
user_account_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $user)

# Set user account hash.
user_account_hash=$(get_account_hash $user_account_key)

# Set deploy dispatch node address. 
node_address=$(get_node_address_rpc $net $node)

# Dispatch approval.
deploy_hash=$(
    $(get_path_to_client $net) put-deploy \
        --chain-name casper-net-$net \
        --gas-price $gas \
        --node-address $node_address \
        --payment-amount $payment \
        --secret-key $contract_owner_secret_key \
        --ttl "1day" \
        --session-name "ERC20" \
        --session-entry-point "approve" \
        --session-arg "spender:account_hash='account-hash-$user_account_hash'" \
        --session-arg "amount:U256='$amount'" \
        | jq '.result.deploy_hash' \
        | sed -e 's/^"//' -e 's/"$//'
)

# Inform.
log "approving ERC20 token transfer"
log "... network = $net"
log "... node = $node"
log "... node address = "$node_address
log "... user account key = "$user_account_key
log "... approved amount = "$amount
log "... deploy hash = "$deploy_hash
