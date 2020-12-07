#!/usr/bin/env bash
#
# Sets assets required to run an N node network.
# Arguments:
#   Network ordinal identifier.
#   Count of nodes to setup.
#   Count of nodes that will be bootstraps.
#   Count of users to setup.
#   Delay in seconds to apply to genesis timestamp.

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

    # Set system contracts.
	for contract in "${NCTL_CONTRACTS_SYSTEM[@]}"
	do
        cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/$contract $1/bin
	done

    # Set client contracts.
	for contract in "${NCTL_CONTRACTS_CLIENT[@]}"
	do
        cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/$contract $1/bin
	done
}

#######################################
# Sets assets pertaining to network chainspec.
# Arguments:
#   Path to network directory.
#   Network ordinal identifier.
#   Delay in seconds to apply to genesis timestamp.
#######################################
function _set_chainspec() {
    log "... chainspec"

    # Set directory.
    mkdir $1/chainspec

    # Set config.
    path_config=$1/chainspec/chainspec.toml
    cp $NCTL_CASPER_HOME/resources/local/chainspec.toml.in $path_config

    # Set config setting: genesis.name.
    GENESIS_NAME=casper-net-$2
    sed -i "s/casper-example/$GENESIS_NAME/g" $path_config > /dev/null 2>&1

    # Set config setting: genesis.timestamp.
    GENESIS_TIMESTAMP=$(get_genesis_timestamp $3)
    sed -i "s/^\([[:alnum:]_]*timestamp\) = .*/\1 = \"${GENESIS_TIMESTAMP}\"/" $path_config > /dev/null 2>&1

    # Override config settings as all paths need to point relative to nctl's assets dir:
    #    genesis.accounts_path
    #    genesis.mint_installer_path
    #    genesis.pos_installer_path
    #    genesis.standard_payment_installer_path
    #    genesis.auction_installer_path
    sed -i "s?\${BASEDIR}/target/wasm32-unknown-unknown/release/?../bin/?g" $path_config > /dev/null 2>&1
    sed -i "s?\${BASEDIR}/resources/local/?./?g" $path_config > /dev/null 2>&1

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
#   NCTL_DAEMON_TYPE - type of daemon service manager.
# Arguments:
#   Path to network directory.
#   Network ordinal identifier.
#   Nodeset count.
#   Boostrap count.
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
        source $NCTL/sh/assets/setup_supervisord.sh $1 $2 $3 $4
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
        $NCTL_INITIAL_BALANCE_FAUCET \
        0
}

#######################################
# Sets assets pertaining to all nodes within network.
# Arguments:
#   Path to network directory.
#   Network ordinal identifier.
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
#   Network ordinal identifier.
#   Node ordinal identifier.
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

    # Set config.
    path_config=$1/nodes/node-$3/config/node-config.toml
    cp $NCTL_CASPER_HOME/resources/local/config.toml $path_config

    # Set keys.
    $1/bin/casper-client keygen -f $1/nodes/node-$3/keys > /dev/null 2>&1

    # Set chainspec account.
    _set_chainspec_account \
        $1 \
        $1/nodes/node-$3/keys/public_key_hex \
        $NCTL_INITIAL_BALANCE_VALIDATOR \
        $(($NCTL_VALIDATOR_BASE_WEIGHT * $3))
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
    for IDX in $(seq 1 $2)
    do
        _set_user $1 $IDX
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
#   Network ordinal identifier.
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
#   Network ordinal identifier.
#   Count of nodes to setup.
#   Count of bootstraps to setup.
#   Count of users to setup.
#   Delay in seconds to apply to genesis timestamp.
#######################################
function _main() {
    # Set directory.
    path_net=$NCTL/assets/net-$1

    # Teardown existing.
    if [ -d $path_net ]; then
        source $NCTL/sh/assets/teardown.sh net=$1
    fi

    log "net-$1: setting up assets ... please wait"

    # Make directory.
    mkdir -p $path_net

    # Set artefacts.
    log "setting network artefacts:"
    _set_bin $path_net
    _set_chainspec $path_net $1 $5
    _set_daemon $path_net $1 $2 $3
    _set_faucet $path_net
    _set_nodes $path_net $1 $2 $3
    _set_users $path_net $4
    _set_vars $path_net $1 $2 $3 $4

    log "net-$1: assets set up"
}

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset BOOTSTRAP_COUNT
unset GENESIS_DELAY_SECONDS
unset NET_ID
unset NODE_COUNT
unset USER_COUNT

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        bootstraps) BOOTSTRAP_COUNT=${VALUE} ;;
        delay) GENESIS_DELAY_SECONDS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        nodes) NODE_COUNT=${VALUE} ;;
        users) USER_COUNT=${VALUE} ;;
        *)
    esac
done

# Set defaults.
BOOTSTRAP_COUNT=${BOOTSTRAP_COUNT:-1}
GENESIS_DELAY_SECONDS=${GENESIS_DELAY_SECONDS:-30}
NET_ID=${NET_ID:-1}
NODE_COUNT=${NODE_COUNT:-5}
USER_COUNT=${USER_COUNT:-5}

#######################################
# Imports
#######################################

# Import utils.
source $NCTL/sh/utils.sh

#######################################
# Main
#######################################

# Execute when inputs are valid.
if [ $BOOTSTRAP_COUNT -ge $NODE_COUNT ]; then
    log_error "Invalid input: bootstraps MUST BE < nodes"
else
    _main $NET_ID $NODE_COUNT $BOOTSTRAP_COUNT $USER_COUNT $GENESIS_DELAY_SECONDS
fi
