#!/usr/bin/env bash
#
# Dispatches wasm transfers to a test net.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   User ordinal identifier.
#   Transfer dispatch count.
#   Transfer dispatch interval.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset amount
unset gas
unset interval
unset net
unset node
unset payment
unset transfers
unset user

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) amount=${VALUE} ;;
        gas) gas=${VALUE} ;;
        interval) transfer_interval=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        payment) payment=${VALUE} ;;
        transfers) transfers=${VALUE} ;;
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
amount=${amount:-$NCTL_DEFAULT_TRANSFER_AMOUNT}
payment=${payment:-$NCTL_DEFAULT_GAS_PAYMENT}
gas=${gas:-$NCTL_DEFAULT_GAS_PRICE}
net=${net:-1}
node=${node:-1}
transfers=${transfers:-100}
transfer_interval=${transfer_interval:-0.01}
user=${user:-1}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Set deploy params.
cp1_secret_key=$(get_path_to_secret_key $net $NCTL_ACCOUNT_TYPE_FAUCET)
cp1_public_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_FAUCET)
cp2_public_key=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $user)
cp2_account_hash=$(get_account_hash $cp2_public_key)
path_contract=$(get_path_to_contract $net "transfer_to_account_u512.wasm")

# Inform.
log "dispatching $transfers wasm transfers"
log "... network=$net"
log "... node=$node"
log "... transfer amount=$amount"
log "... transfer contract=$path_contract"
log "... transfer interval=$transfer_interval (s)"
log "... counter-party 1 public key=$cp1_public_key"
log "... counter-party 2 public key=$cp2_public_key"
log "... counter-party 2 account hash=$cp2_account_hash"
if [ $transfers -le 10 ]; then
    log "... dispatched deploys:"
fi

# Deploy dispatcher.
function _dispatch_deploy() {
    echo $(
        $(get_path_to_client $net) put-deploy \
            --chain-name casper-net-$net \
            --gas-price $gas \
            --node-address $node_address \
            --payment-amount $payment \
            --secret-key $cp1_secret_key \
            --session-arg "amount:u512='$amount'" \
            --session-arg "target:account_hash='account-hash-$cp2_account_hash'" \
            --session-path $path_contract \
            --ttl "1day" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )
}

# Dispatch transfers to each node in round-robin fashion.
if [ $node = "all" ]; then
    transferred=0
    while [ $transferred -lt $transfers ];
    do
        for idx in $(seq 1 $NCTL_NET_NODE_COUNT)
        do
            node_address=$(get_node_address_rpc $net $idx)
            deploy_hash=$(_dispatch_deploy)
            if [ $transfers -le 10 ]; then
                log "... ... "$deploy_hash
            fi
            transferred=$((transferred + 1))
            if [[ $transferred -eq $transfers ]]; then
                break
            fi
            sleep $transfer_interval
        done
    done

# Dispatch transfers to user specified node.
else
    node_address=$(get_node_address_rpc $net $node)
    for transfer_id in $(seq 1 $transfers)
    do
        deploy_hash=$(_dispatch_deploy)
        if [ $transfers -le 10 ]; then
            log "... ... "$deploy_hash
        fi
        sleep $transfer_interval
    done
fi
