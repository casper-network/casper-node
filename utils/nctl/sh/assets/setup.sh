#!/usr/bin/env bash
#
# Sets assets required to run an N node network.
# Arguments:
#   Network ordinal identifier (default=1).
#   Count of nodes to setup (default=5).
#   Delay in seconds to apply to genesis timestamp (default=30).
#   Path to custom chain spec template file.

#######################################
# Imports
#######################################

source "$NCTL"/sh/utils/main.sh

#######################################
# Sets network accounts.toml.
#######################################
function _set_accounts()
{
    local PATH_TO_NET
    local PATH_TO_ACCOUNTS
    local IDX

    # Set accounts.toml.
    PATH_TO_NET=$(get_path_to_net)
    PATH_TO_ACCOUNTS="$PATH_TO_NET"/chainspec/accounts.toml
    touch "$PATH_TO_ACCOUNTS"

    # Set faucet account entry.
    cat >> "$PATH_TO_ACCOUNTS" <<- EOM
# FAUCET.
[[accounts]]
public_key = "$(cat ""$PATH_TO_NET"/faucet/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_FAUCET"
EOM

    # Set validator account entries.
    for IDX in $(seq 1 "$(get_count_of_nodes)")
    do
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# VALIDATOR $IDX.
[[accounts]]
public_key = "$(cat ""$PATH_TO_NET"/nodes/node-"$IDX"/keys/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_VALIDATOR"
EOM
        if [ "$IDX" -le "$(get_count_of_genesis_nodes)" ]; then
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

[accounts.validator]
bonded_amount = "$(_get_node_pos_stake_weight "$IDX")"
delegation_rate = $IDX
EOM
        fi
    done

    # Set user account entries.
    for IDX in $(seq 1 "$(get_count_of_users)")
    do
        if [ "$IDX" -le "$(get_count_of_genesis_nodes)" ]; then
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# USER $IDX.
[[delegators]]
validator_public_key = "$(cat ""$PATH_TO_NET"/nodes/node-"$IDX"/keys/public_key_hex")"
delegator_public_key = "$(cat ""$PATH_TO_NET"/users/user-"$IDX"/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_USER"
delegated_amount = "$((NCTL_INITIAL_DELEGATION_AMOUNT + $IDX))"
EOM
        else
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# USER $IDX.
[[accounts]]
public_key = "$(cat ""$PATH_TO_NET"/users/user-"$IDX"/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_USER"
EOM
        fi
    done
}

#######################################
# Sets network accounts.toml from an existing template.
#######################################
function _set_accounts_from_template()
{
    local ACCOUNT_KEY
    local PATH_TO_ACCOUNTS
    local PATH_TO_TEMPLATE=${1}
    local PBK_KEY
    local IDX

    # Copy across template.    
    PATH_TO_ACCOUNTS="$(get_path_to_net)"/chainspec/accounts.toml
    cp $PATH_TO_TEMPLATE $PATH_TO_ACCOUNTS

    # Set faucet.
    PBK_KEY="PBK_FAUCET"
    ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")"    
    sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"

    # Set validators.
    for IDX in $(seq "$(get_count_of_nodes)" -1 1 )
    do
        PBK_KEY=PBK_V"$IDX"
        ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$IDX")"
        sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"
    done
    
    # Set users.
    for IDX in $(seq "$(get_count_of_users)" -1 1)
    do
        PBK_KEY=PBK_U"$IDX"
        ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" "$IDX")"
        sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"
    done
}

#######################################
# Sets network binaries.
#######################################
function _set_binaries()
{
    local PATH_TO_CLIENT
    local PATH_TO_NET="$(get_path_to_net)"
    local PATH_TO_NODE_BIN
    local PATH_TO_NODE_BIN_SEMVAR

    # Set node binaries.
    for IDX in $(seq 1 "$(get_count_of_nodes)")
    do
        PATH_TO_NODE_BIN=$(get_path_to_node_bin "$IDX")
        PATH_TO_NODE_BIN_SEMVAR="$PATH_TO_NODE_BIN"/1_0_0

        if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
            cp "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/debug/casper-node-launcher" "$PATH_TO_NODE_BIN"
            cp "$NCTL_CASPER_HOME"/target/debug/casper-node "$PATH_TO_NODE_BIN_SEMVAR"
        else
            cp "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/release/casper-node-launcher" "$PATH_TO_NODE_BIN"
            cp "$NCTL_CASPER_HOME"/target/release/casper-node "$PATH_TO_NODE_BIN_SEMVAR"
        fi
    done

    # Set client binaries.
    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cp "$NCTL_CASPER_HOME"/target/debug/casper-client "$PATH_TO_NET"/bin
    else
        cp "$NCTL_CASPER_HOME"/target/release/casper-client "$PATH_TO_NET"/bin
    fi
	for CONTRACT in "${NCTL_CONTRACTS_CLIENT[@]}"
	do
        cp "$NCTL_CASPER_HOME"/target/wasm32-unknown-unknown/release/"$CONTRACT" "$PATH_TO_NET"/bin
	done  
}

#######################################
# Sets network chainspec.
# Arguments:
#   Delay in seconds to apply to genesis timestamp.
#   Path to chainspec template file.
#######################################
function _set_chainspec()
{
    local GENESIS_DELAY=${1}
    local PATH_TO_CHAINSPEC_TEMPLATE=${2}
    local PATH_TO_NET
    local PATH_TO_CHAINSPEC
    local GENESIS_NAME
    local GENESIS_TIMESTAMP
    local SCRIPT

    # Set file.
    PATH_TO_NET=$(get_path_to_net)
    PATH_TO_CHAINSPEC_FILE=$PATH_TO_NET/chainspec/chainspec.toml
    cp "$PATH_TO_CHAINSPEC_TEMPLATE" "$PATH_TO_CHAINSPEC_FILE"

    # Write contents.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CHAINSPEC_FILE');"
        "cfg['protocol']['activation_point']='$(get_genesis_timestamp "$GENESIS_DELAY")';"
        "cfg['network']['name']='$(get_chain_name)';"
        "cfg['core']['validator_slots']=$(($(get_count_of_nodes) * 2));"
        "toml.dump(cfg, open('$PATH_TO_CHAINSPEC_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"   
}

#######################################
# Sets network daemon configuration.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
#######################################
function _set_daemon()
{
    if [ "$NCTL_DAEMON_TYPE" = "supervisord" ]; then
        source "$NCTL"/sh/assets/setup_supervisord.sh
    fi
}

#######################################
# Sets network directories.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#######################################
function _set_directories()
{
    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local PATH_TO_NET="$(get_path_to_net)"
    local PATH_TO_NODE
    local IDX

    mkdir "$PATH_TO_NET"/bin 
    mkdir "$PATH_TO_NET"/chainspec
    mkdir "$PATH_TO_NET"/daemon
    mkdir "$PATH_TO_NET"/daemon/config
    mkdir "$PATH_TO_NET"/daemon/logs
    mkdir "$PATH_TO_NET"/daemon/socket
    mkdir "$PATH_TO_NET"/faucet 
    mkdir "$PATH_TO_NET"/nodes 
    mkdir "$PATH_TO_NET"/users

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        PATH_TO_NODE="$PATH_TO_NET"/nodes/node-"$IDX"
        mkdir "$PATH_TO_NODE"
        mkdir "$PATH_TO_NODE"/bin
        mkdir "$PATH_TO_NODE"/bin/1_0_0
        mkdir "$PATH_TO_NODE"/config
        mkdir "$PATH_TO_NODE"/config/1_0_0
        mkdir "$PATH_TO_NODE"/keys
        mkdir "$PATH_TO_NODE"/logs
        mkdir "$PATH_TO_NODE"/storage
        mkdir "$PATH_TO_NODE"/storage-consensus
    done
     
    for IDX in $(seq 1 "$COUNT_USERS")
    do
        mkdir "$PATH_TO_NET"/users/user-"$IDX"
    done
}

#######################################
# Sets network keys.
#######################################
function _set_keys()
{
    "$(get_path_to_client)" keygen -f "$(get_path_to_net)"/faucet > /dev/null 2>&1
    for IDX in $(seq 1 "$(get_count_of_nodes)")
    do
        "$(get_path_to_client)" keygen -f "$(get_path_to_net)"/nodes/node-"$IDX"/keys > /dev/null 2>&1
    done
    for IDX in $(seq 1 "$(get_count_of_users)")
    do
        "$(get_path_to_client)" keygen -f "$(get_path_to_net)"/users/user-"$IDX" > /dev/null 2>&1
    done
}

#######################################
# Sets network nodes.
#######################################
function _set_nodes()
{
    local IDX
    local PATH_TO_FILE
    local PATH_TO_NODE

    for IDX in $(seq 1 "$(get_count_of_nodes)")
    do
        PATH_TO_CFG=$(get_path_to_node "$IDX")/config/1_0_0
        PATH_TO_FILE="$PATH_TO_CFG"/config.toml

        cp "$NCTL_CASPER_HOME"/resources/local/config.toml "$PATH_TO_CFG"
        cp "$(get_path_to_net)"/chainspec/* "$PATH_TO_CFG"

        local SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_FILE');"
            "cfg['consensus']['secret_key_path']='../../keys/secret_key.pem';"
            "cfg['consensus']['unit_hashes_folder']='../../storage-consensus';"
            "cfg['logging']['format']='$NCTL_NODE_LOG_FORMAT';"
            "cfg['network']['bind_address']='$(get_network_bind_address "$IDX")';"
            "cfg['network']['known_addresses']=[$(get_network_known_addresses "$IDX")];"
            "cfg['storage']['path']='../../storage';"
            "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$IDX")';"
            "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$IDX")';"
            "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$IDX")';"
            "toml.dump(cfg, open('$PATH_TO_FILE', 'w'));"
        )
        python3 -c "${SCRIPT[*]}"
    done
}

#######################################
# Gets a node's default POS weight.
# Arguments:
#   Node ordinal identifier.
#######################################
function _get_node_pos_stake_weight()
{
    local NODE_ID=${1}
    local POS_WEIGHT

    if [ "$NODE_ID" -le "$(get_count_of_genesis_nodes)" ]; then
        POS_WEIGHT=$(get_node_staking_weight "$NODE_ID")
    else
        POS_WEIGHT="0"
    fi
    if [ "x$POS_WEIGHT" = 'x' ]; then
        POS_WEIGHT="0"
    fi

    echo $POS_WEIGHT
}

#######################################
# Main
# Globals:
#   NET_ID - ordinal identifier of network being setup.
# Arguments:
#   Count of nodes to setup.
#   Delay in seconds to apply to genesis timestamp.
#   Path to template chainspec.
#   Path to template accounts.toml.
#######################################
function _main()
{
    local COUNT_NODES=$((${1} * 2))
    local GENESIS_DELAY=${2}
    local CHAINSPEC_PATH=${3}
    local ACCOUNTS_PATH=${4}
    local COUNT_USERS="$COUNT_NODES"
    local PATH_TO_NET

    # Tear down previous.
    PATH_TO_NET=$(get_path_to_net)
    if [ -d "$PATH_TO_NET" ]; then
        source "$NCTL"/sh/assets/teardown.sh net="$NET_ID"
    fi
    mkdir -p "$PATH_TO_NET"

    log "net-$NET_ID: asset setup begins ... please wait"

    # Set artefacts.
    _set_directories "$COUNT_NODES" "$COUNT_USERS"
    _set_binaries
    _set_keys
    _set_daemon
    _set_chainspec "$GENESIS_DELAY" "$CHAINSPEC_PATH"
    if [ "$ACCOUNTS_PATH" = "" ]; then
        _set_accounts
    else
        echo "$ACCOUNTS_PATH"
        _set_accounts_from_template "$ACCOUNTS_PATH"
    fi
    _set_nodes

    log "net-$NET_ID: asset setup complete"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset ACCOUNTS_PATH
unset GENESIS_DELAY_SECONDS
unset NET_ID
unset NODE_COUNT
unset CHAINSPEC_PATH

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        delay) GENESIS_DELAY_SECONDS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        nodes) NODE_COUNT=${VALUE} ;;
        chainspec_path) CHAINSPEC_PATH=${VALUE} ;;
        accounts_path) ACCOUNTS_PATH=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}
GENESIS_DELAY_SECONDS=${GENESIS_DELAY_SECONDS:-30}
NODE_COUNT=${NODE_COUNT:-5}
CHAINSPEC_PATH=${CHAINSPEC_PATH:-"${NCTL_CASPER_HOME}/resources/local/chainspec.toml.in"}
ACCOUNTS_PATH=${ACCOUNTS_PATH:-""}

if [ 3 -gt "$NODE_COUNT" ]; then
    log_error "Invalid input: |nodes| MUST BE >= 3"
else
    _main "$NODE_COUNT" "$GENESIS_DELAY_SECONDS" "$CHAINSPEC_PATH" "$ACCOUNTS_PATH"
fi
