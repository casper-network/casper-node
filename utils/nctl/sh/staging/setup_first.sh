#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset ACCOUNTS_PATH
unset GENESIS_DELAY_SECONDS
unset NET_ID
unset NODE_COUNT

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        accounts_path) ACCOUNTS_PATH=${VALUE} ;;
        delay) GENESIS_DELAY_SECONDS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        nodes) NODE_COUNT=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}
GENESIS_DELAY_SECONDS=${GENESIS_DELAY_SECONDS:-30}
NODE_COUNT=${NODE_COUNT:-5}
ACCOUNTS_PATH=${ACCOUNTS_PATH:-""}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Gets a node's default POS weight.
# Arguments:
#   Count of genesis nodes to setup (default=5).
#   Node ordinal identifier.
#######################################
function _get_node_pos_stake_weight()
{
    local COUNT_NODES_AT_GENESIS=${1}
    local NODE_ID=${2}
    local POS_WEIGHT

    if [ "$NODE_ID" -le "$COUNT_NODES_AT_GENESIS" ]; then
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
# Sets network accounts.toml.
# Arguments:
#   Count of genesis nodes to setup (default=5).
#   Count of all nodes to setup (default=10).
#   Count of users to setup (default=5).
#######################################
function _set_accounts()
{
    log "... setting accounts.toml"

    local COUNT_NODES=${1}
    local COUNT_NODES_AT_GENESIS=${2}
    local COUNT_USERS=${3}

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
public_key = "$(cat "$PATH_TO_NET/faucet/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_FAUCET"
EOM

    # Set validator account entries.
    for IDX in $(seq 1 "$COUNT_NODES")
    do
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# VALIDATOR $IDX.
[[accounts]]
public_key = "$(cat "$PATH_TO_NET/nodes/node-$IDX/keys/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_VALIDATOR"
EOM
        if [ "$IDX" -le "$COUNT_NODES_AT_GENESIS" ]; then
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

[accounts.validator]
bonded_amount = "$(_get_node_pos_stake_weight "$COUNT_NODES_AT_GENESIS" "$IDX")"
delegation_rate = $IDX
EOM
        fi
    done

    # Set user account entries.
    for IDX in $(seq 1 "$COUNT_USERS")
    do
        if [ "$IDX" -le "$COUNT_NODES_AT_GENESIS" ]; then
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# USER $IDX.
[[delegators]]
validator_public_key = "$(cat "$PATH_TO_NET/nodes/node-$IDX/keys/public_key_hex")"
delegator_public_key = "$(cat "$PATH_TO_NET/users/user-$IDX/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_USER"
delegated_amount = "$((NCTL_INITIAL_DELEGATION_AMOUNT + IDX))"
EOM
        else
        cat >> "$PATH_TO_ACCOUNTS" <<- EOM

# USER $IDX.
[[accounts]]
public_key = "$(cat "$PATH_TO_NET/users/user-$IDX/public_key_hex")"
balance = "$NCTL_INITIAL_BALANCE_USER"
EOM
        fi
    done
}

#######################################
# Sets network accounts.toml from an existing template.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#   Path to accounts.toml template file.
#######################################
function _set_accounts_from_template()
{
    log "... setting accounts.toml (from template)"    

    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local PATH_TO_TEMPLATE=${3}

    local ACCOUNT_KEY
    local PATH_TO_ACCOUNTS
    local PBK_KEY
    local IDX

    # Copy across template.    
    PATH_TO_ACCOUNTS="$(get_path_to_net)"/chainspec/accounts.toml
    cp "$PATH_TO_TEMPLATE" "$PATH_TO_ACCOUNTS"

    # Set faucet.
    PBK_KEY="PBK_FAUCET"
    ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")"    
    sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"

    # Set validators.
    for IDX in $(seq "$COUNT_NODES" -1 1 )
    do
        PBK_KEY=PBK_V"$IDX"
        ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$IDX")"
        sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"
    done
    
    # Set users.
    for IDX in $(seq "$COUNT_USERS" -1 1)
    do
        PBK_KEY=PBK_U"$IDX"
        ACCOUNT_KEY="$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" "$IDX")"
        sed -i "s/""$PBK_KEY""/""$ACCOUNT_KEY""/" "$PATH_TO_ACCOUNTS"
    done
}

#######################################
# Sets network binaries.
# Arguments:
#   Count of nodes to setup (default=5).
#   Path to folder containing staged files.
#######################################
function _set_binaries()
{
    log "... setting binaries"

    local COUNT_NODES=${1}
    local PATH_TO_STAGING=${2}

    local PATH_TO_NET="$(get_path_to_net)"
    local PATH_TO_NODE_BIN
    local CONTRACT
    local IDX

    # Set node binaries.
    for IDX in $(seq 1 $COUNT_NODES)
    do
        cp "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/release/casper-node-launcher" "$(get_path_to_node_bin "$IDX")"
        cp "$PATH_TO_STAGING/bin/casper-node" "$(get_path_to_node_bin "$IDX")"/1_0_0
    done

    # Set client binary.
    cp "$PATH_TO_STAGING/bin-client/casper-client" "$PATH_TO_NET/bin"

    # Set client contracts.
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_AUCTION[@]}"
    do
        cp "$PATH_TO_STAGING/bin-client/auction/$CONTRACT" "$PATH_TO_NET/bin/auction"
    done  
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_TRANSFERS[@]}"
    do
        cp "$PATH_TO_STAGING/bin-client/transfers/$CONTRACT" "$PATH_TO_NET/bin/transfers"
    done   
}

#######################################
# Sets network chainspec.
# Arguments:
#   Count of nodes to setup (default=5).
#   Delay in seconds to apply to genesis timestamp.
#   Path to chainspec template file.
#######################################
function _set_chainspec()
{
    log "... setting chainspec.toml"

    local COUNT_NODES=${1}
    local GENESIS_DELAY=${2}
    local PATH_TO_STAGED_CHAINSPEC=${3}

    local PATH_TO_NET_CHAINSPEC="$(get_path_to_net)/chainspec/chainspec.toml"
    local SCRIPT

    # Set file.
    cp "$PATH_TO_STAGED_CHAINSPEC" "$PATH_TO_NET_CHAINSPEC"

    # Write contents.
    SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_NET_CHAINSPEC');"
        "cfg['protocol']['activation_point']='$(get_genesis_timestamp "$GENESIS_DELAY")';"
        "cfg['protocol']['version']='1.0.0';"
        "cfg['network']['name']='$(get_chain_name)';"
        "cfg['core']['validator_slots']=$COUNT_NODES;"
        "toml.dump(cfg, open('$PATH_TO_NET_CHAINSPEC', 'w'));"
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
    log "... setting daemon config"

    if [ "$NCTL_DAEMON_TYPE" = "supervisord" ]; then
        source "$NCTL/sh/assets/setup_supervisord.sh"
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
    log "... setting directories"

    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local PATH_TO_NET="$(get_path_to_net)"
    local PATH_TO_NODE
    local IDX

    mkdir "$PATH_TO_NET/bin"
    mkdir "$PATH_TO_NET/bin/auction"
    mkdir "$PATH_TO_NET/bin/eco"
    mkdir "$PATH_TO_NET/bin/transfers"
    mkdir "$PATH_TO_NET/chainspec"
    mkdir "$PATH_TO_NET/daemon"
    mkdir "$PATH_TO_NET/daemon/config"
    mkdir "$PATH_TO_NET/daemon/logs"
    mkdir "$PATH_TO_NET/daemon/socket"
    mkdir "$PATH_TO_NET/faucet"
    mkdir "$PATH_TO_NET/nodes"
    mkdir "$PATH_TO_NET/users"

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        PATH_TO_NODE="$PATH_TO_NET/nodes/node-$IDX"
        mkdir "$PATH_TO_NODE"
        mkdir "$PATH_TO_NODE/bin"
        mkdir "$PATH_TO_NODE/bin/1_0_0"
        mkdir "$PATH_TO_NODE/config"
        mkdir "$PATH_TO_NODE/config/1_0_0"
        mkdir "$PATH_TO_NODE/keys"
        mkdir "$PATH_TO_NODE/logs"
        mkdir "$PATH_TO_NODE/storage"
        mkdir "$PATH_TO_NODE/storage-consensus"
    done
     
    for IDX in $(seq 1 "$COUNT_USERS")
    do
        mkdir "$PATH_TO_NET/users/user-$IDX"
    done
}

#######################################
# Sets network keys.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#######################################
function _set_keys()
{
    local COUNT_NODES=${1}
    local COUNT_USERS=${2}

    local IDX

    log "... setting cryptographic keys"

    "$(get_path_to_client)" keygen -f "$(get_path_to_net)/faucet" > /dev/null 2>&1

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        "$(get_path_to_client)" keygen -f "$(get_path_to_net)/nodes/node-$IDX/keys" > /dev/null 2>&1
    done
     
    for IDX in $(seq 1 "$COUNT_USERS")
    do
        "$(get_path_to_client)" keygen -f "$(get_path_to_net)/users/user-$IDX" > /dev/null 2>&1
    done
}

#######################################
# Sets network nodes.
# Arguments:
#   Count of nodes to setup (default=5).
#   Path to folder containing staged files.
#######################################
function _set_nodes()
{
    log "... setting node config"
    
    local COUNT_NODES=${1}
    local PATH_TO_STAGING=${2}

    local IDX
    local PATH_TO_NODE_CONFIG
    local PATH_TO_NODE_CONFIG_FILE
    local PATH_TO_STAGED_NODE_CONFIG_FILE="$PATH_TO_STAGING/config/config.toml"

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        PATH_TO_NODE_CONFIG=$(get_path_to_node "$IDX")/config/1_0_0
        PATH_TO_NODE_CONFIG_FILE="$PATH_TO_NODE_CONFIG"/config.toml

        cp "$PATH_TO_STAGED_NODE_CONFIG_FILE" "$PATH_TO_NODE_CONFIG"
        cp "$(get_path_to_net)"/chainspec/* "$PATH_TO_NODE_CONFIG"

        local SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_NODE_CONFIG_FILE');"
            "cfg['consensus']['secret_key_path']='../../keys/secret_key.pem';"
            "cfg['consensus']['unit_hashes_folder']='../../storage-consensus';"
            "cfg['logging']['format']='$NCTL_NODE_LOG_FORMAT';"
            "cfg['network']['bind_address']='$(get_network_bind_address "$IDX")';"
            "cfg['network']['known_addresses']=[$(get_network_known_addresses "$IDX")];"
            "cfg['storage']['path']='../../storage';"
            "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$IDX")';"
            "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$IDX")';"
            "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$IDX")';"
            "toml.dump(cfg, open('$PATH_TO_NODE_CONFIG_FILE', 'w'));"
        )
        python3 -c "${SCRIPT[*]}"
    done
}

function _unset_existing()
{
    local PATH_TO_NET=$(get_path_to_net)

    if [ -d "$PATH_TO_NET" ]; then
        source "$NCTL/sh/assets/teardown.sh" net="$NET_ID"
    fi

    mkdir -p "$PATH_TO_NET"
}

#######################################
# Prepares assets for upgrade scenario start.
# Arguments:
#   Network nodeset count.
#   Delay in seconds pripr to which genesis window will expire.
#   Path to template accounts.toml.
#   Scenario ordinal identifier.
#######################################
function _main()
{
    log "setup: STARTS"

    local COUNT_NODES_AT_GENESIS=${1}
    local COUNT_NODES=$((${1} * 2))
    local GENESIS_DELAY=${2}
    local PATH_TO_ACCOUNTS=${3}

    local COUNT_USERS="$COUNT_NODES"
    local PATH_TO_STAGING="$NCTL/staging/1_0_0"
    local PATH_TO_STAGED_CHAINSPEC="$PATH_TO_STAGING/chainspec/chainspec.toml"

    _unset_existing

    _set_directories "$COUNT_NODES" "$COUNT_USERS"
    _set_binaries "$COUNT_NODES" "$PATH_TO_STAGING"
    _set_keys "$COUNT_NODES" "$COUNT_USERS"
    _set_daemon
    _set_chainspec "$COUNT_NODES" "$GENESIS_DELAY" "$PATH_TO_STAGED_CHAINSPEC"
    if [ "$PATH_TO_ACCOUNTS" = "" ]; then
        _set_accounts "$COUNT_NODES" "$COUNT_NODES_AT_GENESIS" "$COUNT_USERS"
    else
        _set_accounts_from_template "$COUNT_NODES" "$COUNT_USERS" "$PATH_TO_ACCOUNTS"
    fi
    _set_nodes "$COUNT_NODES" "$PATH_TO_STAGING"



    log "setup: COMPLETE"
}

_main "$NODE_COUNT" "$GENESIS_DELAY_SECONDS" "$ACCOUNTS_PATH"
