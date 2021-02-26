#!/usr/bin/env bash
#
# Sets assets required to run an N node network.
# Arguments:
#   Network ordinal identifier (default=1).
#   Count of nodes to setup (default=5).
#   Delay in seconds to apply to genesis timestamp (default=30).

#######################################
# Imports
#######################################

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/assets/setup_node.sh

#######################################
# Sets network bin folder.
#######################################
function _set_net_bin()
{
    local PATH_TO_BIN

    PATH_TO_BIN="$(get_path_to_net)"/bin
    mkdir -p "$PATH_TO_BIN"

    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cp "$NCTL_CASPER_HOME"/target/debug/casper-client "$PATH_TO_BIN"
    else
        cp "$NCTL_CASPER_HOME"/target/release/casper-client "$PATH_TO_BIN"
    fi

	for CONTRACT in "${NCTL_CONTRACTS_CLIENT[@]}"
	do
        cp "$NCTL_CASPER_HOME"/target/wasm32-unknown-unknown/release/"$CONTRACT" "$PATH_TO_BIN"
	done    
}

#######################################
# Sets assets pertaining to network chainspec.
# Arguments:
#   Delay in seconds to apply to genesis timestamp.
#   Number of genesis nodes.
#######################################
function _set_net_chainspec()
{
    local GENESIS_DELAY=${1}
    local COUNT_GENESIS_NODES=${2}
    local PATH_TO_NET
    local PATH_TO_CHAINSPEC
    local GENESIS_NAME
    local GENESIS_TIMESTAMP
    local SCRIPT

    # Set directory.
    PATH_TO_NET=$(get_path_to_net)
    mkdir "$PATH_TO_NET"/chainspec

    # Set file.
    PATH_TO_CHAINSPEC_FILE=$PATH_TO_NET/chainspec/chainspec.toml
    cp "$NCTL_CASPER_HOME"/resources/local/chainspec.toml.in "$PATH_TO_CHAINSPEC_FILE"

    # Write contents.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CHAINSPEC_FILE');"
        "cfg['network']['name']='$(get_chain_name)';"
        "cfg['network']['timestamp']='$(get_genesis_timestamp "$GENESIS_DELAY")';"
        "cfg['core']['validator_slots']=$((COUNT_GENESIS_NODES * 2));"
        "toml.dump(cfg, open('$PATH_TO_CHAINSPEC_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    # Set accounts.toml.
    touch "$PATH_TO_NET"/chainspec/accounts.toml
}

#######################################
# Sets entry in chainspec's accounts.toml.
# Arguments:
#   Path to file containing an ed25519 public key in hex format.
#   Initial account balance (in motes).
#   Initial account staking weight (default=0).
#######################################
function _set_chainspec_account()
{
    local PATH_TO_ACCOUNT_KEY=${1}
    local INITIAL_BALANCE=${2:-0}
    local INITIAL_WEIGHT=${3:-0}
    local ACCOUNT_KEY
    local PATH_TO_NET

    PATH_TO_NET=$(get_path_to_net)
    ACCOUNT_KEY=$(cat "$PATH_TO_ACCOUNT_KEY")

    cat >> "$PATH_TO_NET"/chainspec/accounts.toml <<- EOM
[[accounts]]
public_key = "${ACCOUNT_KEY}"
balance = "$INITIAL_BALANCE"
EOM
    
    if [ "$INITIAL_WEIGHT" != '0' ]; then
        cat >> "$PATH_TO_NET"/chainspec/accounts.toml <<- EOM
[accounts.validator]
bonded_amount = "$INITIAL_WEIGHT"

EOM
    fi
}

#######################################
# Sets entry in chainspec's accounts.toml for a delegator account.
# Arguments:
#   Path to file containing a user's ed25519 public key in hex format.
#   Path to file containing a validator's ed25519 public key in hex format.
#   Initial user account balance.
#   Initial user staking weight.
#######################################
function _set_chainspec_account_for_delegator()
{
    local PATH_TO_ACCOUNT_KEY_OF_USER=${1}
    local PATH_TO_ACCOUNT_KEY_OF_VALIDATOR=${2}
    local BALANCE_OF_USER=${3}
    local WEIGHT_OF_USER=${4:-0}

    local ACCOUNT_KEY_OF_USER
    local ACCOUNT_KEY_OF_VALIDATOR
    local PATH_TO_NET

    PATH_TO_NET=$(get_path_to_net)
    ACCOUNT_KEY_OF_USER=$(cat "$PATH_TO_ACCOUNT_KEY_OF_USER")
    ACCOUNT_KEY_OF_VALIDATOR=$(cat "$PATH_TO_ACCOUNT_KEY_OF_VALIDATOR")

    cat >> "$PATH_TO_NET"/chainspec/accounts.toml <<- EOM
[[delegators]]
balance = "$BALANCE_OF_USER"
delegated_amount = "$WEIGHT_OF_USER"
delegator_public_key = "${ACCOUNT_KEY_OF_USER}"
validator_public_key = "${ACCOUNT_KEY_OF_VALIDATOR}"

EOM
}

#######################################
# Sets entry in chainspec's accounts.toml for a user account.
# Arguments:
#   Path to file containing a user's ed25519 public key in hex format.
#   Initial user account balance.
#######################################
function _set_chainspec_account_for_user()
{
    local PATH_TO_ACCOUNT_KEY_OF_USER=${1}
    local BALANCE_OF_USER=${2}

    local ACCOUNT_KEY_OF_USER
    local PATH_TO_NET

    PATH_TO_NET=$(get_path_to_net)
    ACCOUNT_KEY_OF_USER=$(cat "$PATH_TO_ACCOUNT_KEY_OF_USER")

    cat >> "$PATH_TO_NET"/chainspec/accounts.toml <<- EOM
[[accounts]]
balance = "$BALANCE_OF_USER"
public_key = "${ACCOUNT_KEY_OF_USER}"

EOM
}

#######################################
# Sets assets pertaining to network daemon.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
#######################################
function _set_net_daemon()
{
    local PATH_TO_NET

    # Set directory.
    PATH_TO_NET=$(get_path_to_net)
    mkdir "$PATH_TO_NET"/daemon
    mkdir "$PATH_TO_NET"/daemon/config
    mkdir "$PATH_TO_NET"/daemon/logs
    mkdir "$PATH_TO_NET"/daemon/socket

    # Set daemon specific artefacts.
    if [ "$NCTL_DAEMON_TYPE" = "supervisord" ]; then
        source "$NCTL"/sh/assets/setup_supervisord.sh
    fi
}

#######################################
# Sets assets pertaining to network faucet account.
#######################################
function _set_net_faucet()
{
    local PATH_TO_NET

    # Set directory.
    PATH_TO_NET=$(get_path_to_net)
    mkdir -p "$PATH_TO_NET"/faucet

    # Set keys.
    "$NCTL_CASPER_HOME"/target/release/casper-client keygen -f "$PATH_TO_NET"/faucet > /dev/null 2>&1

    # Set chainspec account.
    _set_chainspec_account \
        "$PATH_TO_NET"/faucet/public_key_hex \
        "$NCTL_INITIAL_BALANCE_FAUCET"
}

#######################################
# Sets assets pertaining to all users within network.
# Arguments:
#   Count of users to setup.
#######################################
function _set_net_users()
{
    local COUNT_USERS=${1}
    local PATH_TO_NET

    # Set directory.
    PATH_TO_NET=$(get_path_to_net)
    mkdir "$PATH_TO_NET"/users

    # Set keys.
    for USER_ID in $(seq 1 "$COUNT_USERS")
    do
        if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
            "$NCTL_CASPER_HOME"/target/debug/casper-client keygen -f "$PATH_TO_NET"/users/user-"$USER_ID" > /dev/null 2>&1
        else
            "$NCTL_CASPER_HOME"/target/release/casper-client keygen -f "$PATH_TO_NET"/users/user-"$USER_ID" > /dev/null 2>&1
        fi    
    done

    # Set user accounts.
    for USER_ID in $(seq 1 "$COUNT_USERS")
    do
        _set_chainspec_account_for_user \
            "$PATH_TO_NET"/users/user-"$USER_ID"/public_key_hex \
            "$NCTL_INITIAL_BALANCE_USER" 
    done
}

#######################################
# Sets assets pertaining to all nodes within network.
# Arguments:
#   Count of genesis nodes to setup.
#######################################
function _set_nodes()
{
    local COUNT_GENESIS_NODES=${1}
    local PATH_TO_NET

    # Set directory.
    PATH_TO_NET=$(get_path_to_net)
    mkdir "$PATH_TO_NET"/nodes

    # We setup assets for twice the number of genesis nodes so that we can subsequently rotate them.
    for NODE_ID in $(seq 1 $((COUNT_GENESIS_NODES * 2)))
    do
        setup_node "$NODE_ID" "$COUNT_GENESIS_NODES"
    done   
}

#######################################
# Sets chainspec assets on a node by node basis.
# Arguments:
#   Count of genesis nodes to setup.
#######################################
function _set_node_chainspecs()
{
    local COUNT_GENESIS_NODES=${1}
    local PATH_TO_NET
    local PATH_TO_NODE

    PATH_TO_NET=$(get_path_to_net)

    for NODE_ID in $(seq 1 $((COUNT_GENESIS_NODES * 2)))
    do
        PATH_TO_NODE=$(get_path_to_node "$NODE_ID")
        cp "$PATH_TO_NET"/chainspec/* "$PATH_TO_NODE"/config/1_0_0
    done
}

#######################################
# Main
# Globals:
#   NET_ID - ordinal identifier of network being setup.
# Arguments:
#   Count of nodes to setup.
#   Delay in seconds to apply to genesis timestamp.
#######################################
function _main()
{
    local COUNT_NODES=${1}
    local GENESIS_DELAY=${2}
    local PATH_TO_NET

    # Tear down previous.
    PATH_TO_NET=$(get_path_to_net)
    if [ -d "$PATH_TO_NET" ]; then
        source "$NCTL"/sh/assets/teardown.sh net="$NET_ID"
    fi
    mkdir -p "$PATH_TO_NET"

    log "net-$NET_ID: asset setup begins ... please wait"

    # Set artefacts.
    log "... setting chainspec"
    _set_net_chainspec "$GENESIS_DELAY" "$COUNT_NODES"

    log "... setting binaries"
    _set_net_bin

    log "... setting faucet"
    _set_net_faucet

    log "... setting nodes"
    _set_nodes "$COUNT_NODES"

    log "... setting users"
    _set_net_users "$COUNT_NODES"

    log "... setting daemon"
    _set_net_daemon

    log "... setting node chainspecs"
    _set_node_chainspecs "$COUNT_NODES"

    log "net-$NET_ID: asset setup complete"
}

#######################################
# Destructure input args.
#######################################

unset GENESIS_DELAY_SECONDS
unset NET_ID
unset NODE_COUNT

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        delay) GENESIS_DELAY_SECONDS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        nodes) NODE_COUNT=${VALUE} ;;
        *)
    esac
done


export NET_ID=${NET_ID:-1}
GENESIS_DELAY_SECONDS=${GENESIS_DELAY_SECONDS:-30}
NODE_COUNT=${NODE_COUNT:-5}

if [ 3 -gt "$NODE_COUNT" ]; then
    log_error "Invalid input: |nodes| MUST BE >= 3"
else
    _main "$NODE_COUNT" "$GENESIS_DELAY_SECONDS"
fi
