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
    local PATH_TO_NET=${1}

    log "... binaries"

    # Set directory.
    mkdir $PATH_TO_NET/bin

    # Set executables.
    cp $NCTL_CASPER_HOME/target/release/casper-client $PATH_TO_NET/bin
    cp $NCTL_CASPER_HOME/target/release/casper-node $PATH_TO_NET/bin

    # Set system contracts.
	for contract in "${NCTL_CONTRACTS_SYSTEM[@]}"
	do
        cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/$contract $PATH_TO_NET/bin
	done

    # Set client contracts.
	for contract in "${NCTL_CONTRACTS_CLIENT[@]}"
	do
        cp $NCTL_CASPER_HOME/target/wasm32-unknown-unknown/release/$contract $PATH_TO_NET/bin
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
    local PATH_TO_NET=${1}
    local NET_ID=${2}
    local GENESIS_DELAY=${3}
    local PATH_TO_CHAINSPEC=$PATH_TO_NET/chainspec/chainspec.toml

    log "... chainspec"

    # Set directory.
    mkdir $PATH_TO_NET/chainspec

    # Set config.
    cp $NCTL_CASPER_HOME/resources/local/chainspec.toml.in $PATH_TO_CHAINSPEC

    # Set config setting: genesis.name.
    GENESIS_NAME=$(get_chain_name $NET_ID)
    sed -i "s/casper-example/$GENESIS_NAME/g" $PATH_TO_CHAINSPEC > /dev/null 2>&1

    # Set config setting: genesis.timestamp.
    GENESIS_TIMESTAMP=$(get_genesis_timestamp $GENESIS_DELAY)
    sed -i "s/^\([[:alnum:]_]*timestamp\) = .*/\1 = \"${GENESIS_TIMESTAMP}\"/" $PATH_TO_CHAINSPEC > /dev/null 2>&1

    # Override config settings as all paths need to point relative to nctl's assets dir:
    #    genesis.accounts_path
    #    genesis.mint_installer_path
    #    genesis.pos_installer_path
    #    genesis.standard_payment_installer_path
    #    genesis.auction_installer_path    
    sed -i "s?\${BASEDIR}/target/wasm32-unknown-unknown/release/?../bin/?g" $PATH_TO_CHAINSPEC > /dev/null 2>&1
    sed -i "s?\${BASEDIR}/resources/local/?./?g" $PATH_TO_CHAINSPEC > /dev/null 2>&1

    # Set accounts.csv.
    touch $PATH_TO_NET/chainspec/accounts.csv
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
    local PATH_TO_NET=${1}
    local PATH_TO_ACCOUNT_KEY=${2}
    local INITIAL_BALANCE=${3}
    local INITIAL_WEIGHT=${4}

    account_key=`cat $PATH_TO_ACCOUNT_KEY`
	cat >> $PATH_TO_NET/chainspec/accounts.csv <<- EOM
	${account_key},$INITIAL_BALANCE,$INITIAL_WEIGHT
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
    local PATH_TO_NET=${1}
    local NET_ID=${2}
    local COUNT_NODES=${3}
    local COUNT_BOOTSTRAPS=${4}

    log "... daemon"

    # Set directory.
    mkdir $PATH_TO_NET/daemon
    mkdir $PATH_TO_NET/daemon/config
    mkdir $PATH_TO_NET/daemon/logs
    mkdir $PATH_TO_NET/daemon/socket

    # Set daemon specific artefacts.
    if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
        source $NCTL/sh/assets/setup_supervisord.sh $PATH_TO_NET $NET_ID $COUNT_NODES $COUNT_BOOTSTRAPS
    fi
}

#######################################
# Sets assets pertaining to network faucet account.
# Arguments:
#   Path to network directory.
#######################################
function _set_faucet() {
    local PATH_TO_NET=${1}

    log "... faucet"

    # Set directory.
    mkdir $PATH_TO_NET/faucet

    # Set keys.
    $PATH_TO_NET/bin/casper-client keygen -f $PATH_TO_NET/faucet > /dev/null 2>&1

    # Set chainspec account.
    _set_chainspec_account \
        $PATH_TO_NET \
        $PATH_TO_NET/faucet/public_key_hex \
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
    local PATH_TO_NET=${1}
    local NET_ID=${2}
    local COUNT_NODES=${3}
    local COUNT_BOOTSTRAPS=${4}

    log "... nodes"
    mkdir $PATH_TO_NET/nodes
    for IDX in $(seq 1 $(($COUNT_NODES * 2)))
    do
        _set_node $PATH_TO_NET $NET_ID $IDX $COUNT_NODES
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
    local PATH_TO_NET=${1}
    local NET_ID=${2}
    local NODE_ID=${3}
    local COUNT_NODES=${4}

    # Set directory.
    mkdir $PATH_TO_NET/nodes/node-$NODE_ID
    mkdir $PATH_TO_NET/nodes/node-$NODE_ID/config
    mkdir $PATH_TO_NET/nodes/node-$NODE_ID/keys
    mkdir $PATH_TO_NET/nodes/node-$NODE_ID/logs
    mkdir $PATH_TO_NET/nodes/node-$NODE_ID/storage

    # Set config.
    cp $NCTL_CASPER_HOME/resources/local/config.toml \
       $PATH_TO_NET/nodes/node-$NODE_ID/config/node-config.toml

    # Set keys.
    $PATH_TO_NET/bin/casper-client keygen -f $PATH_TO_NET/nodes/node-$NODE_ID/keys > /dev/null 2>&1

    # Set chainspec account.
    if [ $NODE_ID -le $COUNT_NODES ]; then
        # ... genesis validator set get staking weight as well as initial balance
        _set_chainspec_account \
            $PATH_TO_NET \
            $PATH_TO_NET/nodes/node-$NODE_ID/keys/public_key_hex \
            $NCTL_INITIAL_BALANCE_VALIDATOR \
            $(($NCTL_VALIDATOR_BASE_WEIGHT * $NODE_ID))
    else
        # ... non-genesis validator set only get initial balance
        _set_chainspec_account \
            $PATH_TO_NET \
            $PATH_TO_NET/nodes/node-$NODE_ID/keys/public_key_hex \
            $NCTL_INITIAL_BALANCE_VALIDATOR \
            0    
    fi
}

#######################################
# Sets assets pertaining to all users within network.
# Arguments:
#   Path to network directory.
#   Count of users to setup.
#######################################
function _set_users() {
    local PATH_TO_NET=${1}
    local COUNT_USERS=${2}

    log "... users"
    mkdir $PATH_TO_NET/users
    for IDX in $(seq 1 $COUNT_USERS)
    do
        $PATH_TO_NET/bin/casper-client keygen -f $PATH_TO_NET/users/user-$IDX > /dev/null 2>&1
        _set_chainspec_account \
            $PATH_TO_NET \
            $PATH_TO_NET/users/user-$IDX/public_key_hex \
            $NCTL_INITIAL_BALANCE_VALIDATOR \
            0    
    done
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
    local PATH_TO_NET=${1}
    local NET_ID=${2}
    local COUNT_NODES=${3}
    local COUNT_BOOTSTRAPS=${4}
    local COUNT_USERS=${5}

    log "... variables"

    touch $PATH_TO_NET/vars
	cat >> $PATH_TO_NET/vars <<- EOM
# Count of nodes to setup.
export NCTL_NET_BOOTSTRAP_COUNT=$COUNT_BOOTSTRAPS

# Network ordinal identifier.
export NCTL_NET_IDX=$NET_ID

# Count of nodes to setup.
export NCTL_NET_NODE_COUNT=$COUNT_NODES

# Count of users to setup.
export NCTL_NET_USER_COUNT=$COUNT_USERS
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
    local NET_ID=${1}
    local COUNT_NODES=${2}
    local COUNT_BOOTSTRAPS=${3}
    local COUNT_USERS=${4}
    local GENESIS_DELAY=${5}

    # Set directory.
    PATH_TO_NET=$(get_path_to_net $NET_ID)

    # Teardown existing.
    if [ -d $PATH_TO_NET ]; then
        source $NCTL/sh/assets/teardown.sh net=$NET_ID
    fi

    log "net-$NET_ID: setting up assets ... please wait"

    # Make directory.
    mkdir -p $PATH_TO_NET

    # Set vars.
    _set_vars $PATH_TO_NET $NET_ID $COUNT_NODES $COUNT_BOOTSTRAPS $COUNT_USERS

    # Set artefacts.
    log "setting network artefacts:"
    _set_bin $PATH_TO_NET
    _set_chainspec $PATH_TO_NET $NET_ID $GENESIS_DELAY
    _set_daemon $PATH_TO_NET $NET_ID $COUNT_NODES $COUNT_BOOTSTRAPS
    _set_faucet $PATH_TO_NET
    _set_nodes $PATH_TO_NET $NET_ID $COUNT_NODES $COUNT_BOOTSTRAPS
    _set_users $PATH_TO_NET $COUNT_USERS

    log "net-$NET_ID: assets set up"
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
