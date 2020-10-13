#!/usr/bin/env bash
#
# Sets assets required to run an N node network.
# Arguments:
#   Network ordinal identifer.
#   Count of nodes to setup.
#   Count of users to setup.

#######################################
# Sets assets pertaining to network binaries.
# Globals:
#   NCTL_CASPER_HOME - path to node software github repo.
# Arguments:
#   Path to network directory.
#######################################
function _set_bin() {
    log "... binaries"

    # Set directory.
    mkdir $1/bin

    # Set executables.
    cp $NCTL_CASPER_HOME/target/release/casper-client $1/bin
    cp $NCTL_CASPER_HOME/target/release/casper-node $1/bin

    # Set wasm:
    # ... system contracts;
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/auction_install.wasm $1/bin
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/mint_install.wasm $1/bin
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/pos_install.wasm $1/bin
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/standard_payment_install.wasm $1/bin
    # ... client contracts.
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/add_bid.wasm $1/bin
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/delegate.wasm $1/bin
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/transfer_to_account_u512.wasm $1/bin
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/transfer_to_account_u512_stored.wasm $1/bin
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/undelegate.wasm $1/bin
    cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/withdraw_bid.wasm $1/bin
}

#######################################
# Sets assets pertaining to network chainspec.
# Arguments:
#   Path to network directory.
#   Network ordinal identifer.
#######################################
function _set_chainspec() {
    log "... chainspec"

    # Set directory.
    mkdir $1/chainspec

    # Set config params.
    GENESIS_NAME=casper-net-$2
    GENESIS_TIMESTAMP=$(python3 -c 'from datetime import datetime, timedelta; print((datetime.utcnow() + timedelta(seconds=20)).isoformat("T") + "Z")')
    HIGHWAY_GENESIS_ERA_START_TIMESTAMP=$(python3 -c 'from datetime import datetime, timedelta; print((datetime.utcnow() + timedelta(seconds=20)).isoformat("T") + "Z")')

    # Set config.
    path_config=$1/chainspec/chainspec.toml
    cp $NCTL/templates/chainspec.toml $path_config
    sed -i "s/{GENESIS_NAME}/$GENESIS_NAME/g" $path_config > /dev/null 2>&1
    sed -i "s/{GENESIS_TIMESTAMP}/$GENESIS_TIMESTAMP/g" $path_config > /dev/null 2>&1
    sed -i "s/{HIGHWAY_GENESIS_ERA_START_TIMESTAMP}/$HIGHWAY_GENESIS_ERA_START_TIMESTAMP/g" $path_config > /dev/null 2>&1

    # Set accounts.csv.
    touch $1/chainspec/accounts.csv
}

#######################################
# Sets entry in chainspec's accounts.csv.
# Arguments:
#   Path to network directory.
#   Path to file containing an ed25519 public key in hex format.
#   Initial account balance (in motes).
#   Staking weight - validator's only.
#######################################
function _set_chainspec_account() {
    public_key_hex=`cat $2`
	cat >> $1/chainspec/accounts.csv <<- EOM
	${public_key_hex},$3,$4
	EOM
}

#######################################
# Sets assets pertaining to network daemon.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Path to network directory.
#   Network ordinal identifer.
#   Nodeset count.
#######################################
function _set_daemon() {
    log "... daemon"

    # Set directory.
    mkdir $1/daemon
    mkdir $1/daemon/config
    mkdir $1/daemon/logs
    mkdir $1/daemon/socket

    # Set daemon specific artefacts.
    if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
        source $NCTL/sh/assets/setup_supervisord.sh $1 $2 $3
    fi
}

#######################################
# Sets assets pertaining to network faucet account.
# Arguments:
#   Path to network directory.
#######################################
function _set_faucet() {
    log "... faucet"

    # Set directory.
    mkdir $1/faucet

    # Set keys.
    $1/bin/casper-client keygen -f $1/faucet > /dev/null 2>&1

    # Set chainspec account.
    _set_chainspec_account \
        $1 \
        $1/faucet/public_key_hex \
        100000000000000000 \
        0
}

#######################################
# Sets assets pertaining to all nodes within network.
# Arguments:
#   Path to network directory.
#   Network ordinal identifer.
#   Count of nodes to setup.
#   Count of bootstraps to setup.
#######################################
function _set_nodes() {
    log "... nodes"

    mkdir $1/nodes
    for node_id in $(seq 1 $3)
    do
        _set_node $1 $2 $node_id $4
    done
}

#######################################
# Sets assets pertaining to a single node.
# Arguments:
#   Path to network directory.
#   Network ordinal identifer.
#   Node ordinal identifer.
#   Count of bootstraps to setup.
#######################################
function _set_node ()
{
    # Set directory.
    mkdir $1/nodes/node-$3
    mkdir $1/nodes/node-$3/config
    mkdir $1/nodes/node-$3/keys
    mkdir $1/nodes/node-$3/logs
    mkdir $1/nodes/node-$3/storage

    # Set keys.
    $1/bin/casper-client keygen -f $1/nodes/node-$3/keys > /dev/null 2>&1

    # Set config params.
    HTTP_SERVER_BIND_PORT=$((50000 + ($2 * 100) + $node_id))
    NETWORK_BIND_PORT=0
    if [ $3 -le $4 ]; then
        NETWORK_BIND_PORT=$((34452 + ($2 * 100) + $node_id))
        NETWORK_KNOWN_ADDRESSES=""
    else
        NETWORK_BIND_PORT=0
        NETWORK_KNOWN_ADDRESSES="$(get_bootstrap_known_addresses $2 $4)"
    fi

    # Set config.
    path_config=$1/nodes/node-$3/config/node-config.toml
    cp $NCTL/templates/node-config.toml $path_config
    sed -i "s/{NETWORK_BIND_PORT}/$NETWORK_BIND_PORT/g" $path_config > /dev/null 2>&1
    sed -i "s/{NETWORK_KNOWN_ADDRESSES}/$NETWORK_KNOWN_ADDRESSES/g" $path_config > /dev/null 2>&1
    sed -i "s/{HTTP_SERVER_BIND_PORT}/$HTTP_SERVER_BIND_PORT/g" $path_config > /dev/null 2>&1

    # Set chainspec account.
    _set_chainspec_account \
        $1 \
        $1/nodes/node-$3/keys/public_key_hex \
        100000000000000000 \
        $((100000000000000 * $3))
}

#######################################
# Get network known addresses - i.e. those of bootstrap nodes.
# Arguments:
#   Network ordinal identifer.
#   Count of bootstraps to setup.
#######################################
function get_bootstrap_known_addresses() {
    result=""

    for bootstrap_idx in $(seq 1 $2)
    do
        address=$(get_bootstrap_known_address $1 $bootstrap_idx)
        result=$result"'"$address"',"
    done

    echo $result
}

#######################################
# Get a network known addresses - i.e. those of bootstrap nodes.
# Arguments:
#   Network ordinal identifer.
#   Node ordinal identifer.
#######################################
function get_bootstrap_known_address() {
    port=$((34452 + ($1 * 100) + $2))
    address="127.0.0.1:"$port

    echo $address
}

#######################################
# Sets assets pertaining to all users within network.
# Arguments:
#   Path to network directory.
#   Count of users to setup.
#######################################
function _set_users() {
    log "... users"

    mkdir $1/users
    for user_idx in $(seq 1 $2)
    do
        _set_user $1 $user_idx
    done
}

#######################################
# Sets assets pertaining to a single user.
# Arguments:
#   Path to network directory.
#   Path to user directory.
#######################################
function _set_user() {
    $1/bin/casper-client keygen -f $1/users/user-$2 > /dev/null 2>&1
}

#######################################
# Sets assets pertaining to network variables.
# Arguments:
#   Path to network directory.
#   Network ordinal identifer.
#   Count of nodes to setup.
#   Count of bootstraps to setup.
#   Count of users to setup.
#######################################
function _set_vars() {
    log "... variables"

    touch $1/vars
	cat >> $1/vars <<- EOM
# Count of nodes to setup.
export NCTL_NET_BOOTSTRAP_COUNT=$4

# Network ordinal identifier.
export NCTL_NET_IDX=$2

# Count of nodes to setup.
export NCTL_NET_NODE_COUNT=$3

# Count of users to setup.
export NCTL_NET_USER_COUNT=$5
	EOM
}

#######################################
# Main
# Arguments:
#   Network ordinal identifer.
#   Count of nodes to setup.
#   Count of bootstraps to setup.
#   Count of users to setup.
#######################################
function _main() {
    # Set directory.
    net_path=$NCTL/assets/net-$1

    # Teardown existing.
    if [ -d $net_path ]; then
        source $NCTL/sh/assets/teardown.sh net=$1
    fi

    log "network #$1: setting up assets ... please wait"

    # Make directory.
    mkdir -p $net_path

    # Set artefacts.
    log "setting network artefacts:"
    _set_bin $net_path
    _set_chainspec $net_path $1
    _set_daemon $net_path $1 $2
    _set_faucet $net_path
    _set_nodes $net_path $1 $2 $3
    _set_users $net_path $4
    _set_vars $net_path $1 $2 $3 $4

    log "network #$1: assets set up"
}

#######################################
# CLI entry point
# Arguments:
#   Network ordinal identifer.
#   Count of nodes to setup.
#   Count of users to setup.
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset bootstraps
unset net
unset nodes
unset users

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        bootstraps) bootstraps=${VALUE} ;;
        net) net=${VALUE} ;;
        nodes) nodes=${VALUE} ;;
        users) users=${VALUE} ;;
        *)
    esac
done

# Set defaults.
bootstraps=${bootstraps:-1}
net=${net:-1}
nodes=${nodes:-5}
users=${users:-5}

#######################################
# Main
#######################################

# Execute when inputs are valid.
if [ $bootstraps -ge $nodes ]; then
    log_error "Invalid input: bootstraps MUST BE < nodes"
else
    _main $net $nodes $bootstraps $users
fi
