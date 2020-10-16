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

# Import utils.
source $NCTL/sh/utils/misc.sh

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
        gas) gas_price=${VALUE} ;;
        interval) transfer_interval=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        payment) gas_payment=${VALUE} ;;
        transfers) transfers=${VALUE} ;;
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
amount=${amount:-1000000}
gas_payment=${gas_payment:-200000}
gas_price=${gas_price:-10}
net=${net:-1}
node=${node:-1}
transfers=${transfers:-100}
transfer_interval=${transfer_interval:-0.01}
user=${user:-1}

#######################################
# Main
#######################################

# Set paths.
path_net=$NCTL/assets/net-$net
path_contract=$path_net/bin/transfer_to_account_u512.wasm

# Set counter-parties.
cp1_secret_key=$path_net/faucet/secret_key.pem
cp1_public_key=`cat $path_net/faucet/public_key_hex`
cp2_public_key=`cat $path_net/users/user-$user/public_key_hex`
cp2_account_hash=$(get_hash $cp2_public_key)

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

# Dispatch transfers to each node in round-robin fashion.
if [ $node = "all" ]; then
    transferred=0
    while [ $transferred -lt $transfers ];
    do
        source $NCTL/assets/net-$net/vars
        for node_idx in $(seq 1 $NCTL_NET_NODE_COUNT)
        do
            $path_net/bin/casper-client put-deploy \
                --chain-name casper-net-$net \
                --gas-price $gas_price \
                --node-address $(get_node_address $net $node_idx) \
                --payment-amount $gas_payment \
                --secret-key $cp1_secret_key \
                --session-arg "amount:u512='$amount'" \
                --session-arg "target:account_hash='account-hash-$cp2_account_hash'" \
                --session-path $path_contract \
                --ttl "1day" \
                > /dev/null 2>&1

            transferred=$((transferred + 1))
            if [[ $transferred -eq $transfers ]]; then
                break
            fi

            sleep $transfer_interval
        done
    done

# Dispatch transfers to user specified node.
else
    node_address=$(get_node_address $net $node)
    for transfer_id in $(seq 1 $transfers)
    do
        $path_net/bin/casper-client put-deploy \
            --chain-name casper-net-$net \
            --gas-price $gas_price \
            --node-address $(get_node_address $net $node_idx) \
            --payment-amount $gas_payment \
            --secret-key $cp1_secret_key \
            --session-arg "amount:u512='$amount'" \
            --session-arg "target:account_hash='account-hash-$cp2_account_hash'" \
            --session-path $path_contract \
            --ttl "1day" \
            > /dev/null 2>&1
        sleep $transfer_interval
    done
fi

log "dispatch complete"
