#!/usr/bin/env bash

#######################################
# Gets a node's default POS weight.
# Arguments:
#   Count of genesis nodes to setup (default=5).
#   Node ordinal identifier.
#######################################
function get_node_pos_stake_weight()
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
function setup_asset_accounts()
{
    log "... setting accounts.toml"

    local COUNT_NODES=${1}
    local COUNT_NODES_AT_GENESIS=${2}
    local COUNT_USERS=${3}
    local IDX
    local PATH_TO_ACCOUNTS
    local PATH_TO_NET

    local PATH_TO_ACCOUNTS="$PATH_TO_NET/chainspec/accounts.toml"

    PATH_TO_NET="$(get_path_to_net)"
    PATH_TO_ACCOUNTS="$PATH_TO_NET/chainspec/accounts.toml"

    # Set accounts.toml.
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
bonded_amount = "$(get_node_pos_stake_weight "$COUNT_NODES_AT_GENESIS" "$IDX")"
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
function setup_asset_accounts_from_template()
{
    log "... setting accounts.toml (from template)"

    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local PATH_TO_TEMPLATE=${3}

    local ACCOUNT_KEY
    local PATH_TO_ACCOUNTS="$PATH_TO_NET/chainspec/accounts.toml"
    local PBK_KEY
    local IDX

    # Copy across template.
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
#   Version of protocol being run.
#   Count of nodes to setup (default=5).
#   Path to casper-client binary.
#   Path to casper-node binary.
#   Path to casper-node-launcher binary.
#   Path to folder containing wasm binaries.
#######################################
function setup_asset_binaries()
{
    log "... setting binaries"

    local PROTOCOL_VERSION=${1}
    local COUNT_NODES=${2}
    local PATH_TO_CLIENT=${3}
    local PATH_TO_NODE=${4}
    local PATH_TO_NODE_LAUNCHER=${5}
    local PATH_TO_WASM=${6}

    local PATH_TO_BIN
    local CONTRACT
    local IDX

    # Set node binaries.
    for IDX in $(seq 1 "$COUNT_NODES")
    do
        PATH_TO_BIN="$(get_path_to_node_bin "$IDX")"
        if [ ! -f "$PATH_TO_BIN/casper-node-launcher" ]; then
            cp "$PATH_TO_NODE_LAUNCHER" "$PATH_TO_BIN"
        fi
        cp "$PATH_TO_NODE" "$PATH_TO_BIN/$PROTOCOL_VERSION"
    done

    # Set client-side binary.
    PATH_TO_BIN="$(get_path_to_net)/bin"
    cp "$PATH_TO_CLIENT" "$PATH_TO_BIN"

    # Set client-side auction contracts;
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_AUCTION[@]}"
    do
        if [ -f "$PATH_TO_WASM/$CONTRACT" ]; then
            cp "$PATH_TO_WASM/$CONTRACT" \
               "$PATH_TO_BIN/auction"
        fi
    done

    # Set client-side shared contracts;
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_SHARED[@]}"
    do
        if [ -f "$PATH_TO_WASM/$CONTRACT" ]; then
            cp "$PATH_TO_WASM/$CONTRACT" \
            "$PATH_TO_BIN/shared"
        fi
    done

    # Set client-side transfer contracts;
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_TRANSFERS[@]}"
    do
        if [ -f "$PATH_TO_WASM/$CONTRACT" ]; then
            cp "$PATH_TO_WASM/$CONTRACT" \
            "$PATH_TO_BIN/transfers"
        fi
    done
}

#######################################
# Sets network chainspec.
# Arguments:
#   Count of nodes to setup (default=5).
#   Point (timestamp | era-id) when chainspec is considered live.
#   Delay in seconds to apply to genesis timestamp.
#   Path to chainspec template file.
#   Flag indicating whether chainspec pertains to genesis.
#######################################
function setup_asset_chainspec()
{
    log "... setting chainspec.toml"

    local COUNT_NODES=${1}
    local PROTOCOL_VERSION=${2}
    local ACTIVATION_POINT=${3}
    local PATH_TO_CHAINSPEC_TEMPLATE=${4}
    local IS_GENESIS=${5}
    local CHUNKED_HASH_ACTIVATION=${6}
    local PATH_TO_CHAINSPEC
    local SCRIPT

    # Set file.
    PATH_TO_CHAINSPEC="$(get_path_to_net)/chainspec/chainspec.toml"
    cp "$PATH_TO_CHAINSPEC_TEMPLATE" "$PATH_TO_CHAINSPEC"

    # Set contents.
    if [ $IS_GENESIS == true ]; then
        SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_CHAINSPEC');"
            "cfg['protocol']['activation_point']='$ACTIVATION_POINT';"
            "cfg['protocol']['version']='$PROTOCOL_VERSION';"
            "cfg['network']['name']='$(get_chain_name)';"
            "cfg['core']['validator_slots']=$COUNT_NODES;"
            "toml.dump(cfg, open('$PATH_TO_CHAINSPEC', 'w'));"
        )
    else
        SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_CHAINSPEC');"
            "cfg['protocol']['activation_point']=$ACTIVATION_POINT;"
            "cfg['protocol']['version']='$PROTOCOL_VERSION';"
            "cfg['protocol']['verifiable_chunked_hash_activation']=$CHUNKED_HASH_ACTIVATION;"
            "cfg['network']['name']='$(get_chain_name)';"
            "cfg['core']['validator_slots']=$COUNT_NODES;"
            "toml.dump(cfg, open('$PATH_TO_CHAINSPEC', 'w'));"
        )
    fi

    python3 -c "${SCRIPT[*]}"
}

#######################################
# Sets network daemon configuration.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
#######################################
function setup_asset_daemon()
{
    log "... setting daemon config"

    if [ "$NCTL_DAEMON_TYPE" = "supervisord" ]; then
        source "$NCTL"/sh/assets/setup_supervisord.sh
    fi
}

#######################################
# Sets network directories.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#   Initial protocol version under which network will spinup.
#######################################
function setup_asset_directories()
{
    log "... setting directories"

    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local PROTOCOL_VERSION_INITIAL=${3}
    local PATH_TO_NET
    local PATH_TO_NODE
    local IDX

    PATH_TO_NET="$(get_path_to_net)"

    mkdir "$PATH_TO_NET/bin"
    mkdir "$PATH_TO_NET/bin/auction"
    mkdir "$PATH_TO_NET/bin/eco"
    mkdir "$PATH_TO_NET/bin/shared"
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
        PATH_TO_NODE="$PATH_TO_NET"/nodes/node-"$IDX"
        mkdir "$PATH_TO_NODE"
        mkdir "$PATH_TO_NODE/bin"
        mkdir "$PATH_TO_NODE/bin/$PROTOCOL_VERSION_INITIAL"
        mkdir "$PATH_TO_NODE/config"
        mkdir "$PATH_TO_NODE/config/$PROTOCOL_VERSION_INITIAL"
        mkdir "$PATH_TO_NODE/keys"
        mkdir "$PATH_TO_NODE/logs"
        mkdir "$PATH_TO_NODE/storage"
    done

    for IDX in $(seq 1 "$COUNT_USERS")
    do
        mkdir "$PATH_TO_NET"/users/user-"$IDX"
    done
}

#######################################
# Sets network keys.
# Arguments:
#   Count of nodes to setup (default=5).
#   Count of users to setup (default=5).
#######################################
function setup_asset_keys()
{
    local COUNT_NODES=${1}
    local COUNT_USERS=${2}
    local IDX
    local PATH_TO_CLIENT
    local PATH_TO_NET

    PATH_TO_CLIENT="$(get_path_to_client)"
    PATH_TO_NET="$(get_path_to_net)"

    log "... setting cryptographic keys"

    "$PATH_TO_CLIENT" keygen -f "$PATH_TO_NET/faucet" > /dev/null 2>&1

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        "$PATH_TO_CLIENT" keygen -f "$PATH_TO_NET/nodes/node-$IDX/keys" > /dev/null 2>&1
    done

    for IDX in $(seq 1 "$COUNT_USERS")
    do
        "$PATH_TO_CLIENT" keygen -f "$PATH_TO_NET/users/user-$IDX" > /dev/null 2>&1
    done
}

#######################################
# Sets node confgiuration files.
# Arguments:
#   Count of nodes to setup (default=5).
#   Version of protocol being ran.
#   Path to node configuration template file.
#   Flag indicating whether chainspec pertains to genesis.
#######################################
function setup_asset_node_configs()
{
    log "... setting node configs"

    local COUNT_NODES=${1}
    local PROTOCOL_VERSION=${2}
    local PATH_TO_TEMPLATE=${3}
    local IS_GENESIS=${4}

    local IDX
    local PATH_TO_NET
    local PATH_TO_CONFIG
    local PATH_TO_CONFIG_FILE
    local SCRIPT

    PATH_TO_NET="$(get_path_to_net)"

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        # Set paths to node's config.
        PATH_TO_CONFIG="$(get_path_to_node "$IDX")/config/$PROTOCOL_VERSION"
        PATH_TO_CONFIG_FILE="$PATH_TO_CONFIG/config.toml"

        # Set node configuration.
        if [ $IS_GENESIS == true ]; then
            cp "$PATH_TO_NET/chainspec/accounts.toml" "$PATH_TO_CONFIG"
        fi
        cp "$PATH_TO_NET/chainspec/chainspec.toml" "$PATH_TO_CONFIG"
        cp "$PATH_TO_TEMPLATE" "$PATH_TO_CONFIG_FILE"

        # Set node configuration settings.
        SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_CONFIG_FILE');"
            "cfg['consensus']['secret_key_path']='../../keys/secret_key.pem';"
            "cfg['logging']['format']='$NCTL_NODE_LOG_FORMAT';"
            "cfg['network']['bind_address']='$(get_network_bind_address "$IDX")';"
            "cfg['network']['known_addresses']=[$(get_network_known_addresses "$IDX")];"
            "cfg['storage']['path']='../../storage';"
            "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$IDX")';"
            "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$IDX")';"
            "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$IDX")';"
            "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
        )
        python3 -c "${SCRIPT[*]}"
    done
}

function setup_asset_global_state_toml() {
    log "... setting node global_state.toml"

    local COUNT_NODES=${1}
    local PROTOCOL_VERSION=${2}
    local IDX
    local GLOBAL_STATE_OUTPUT
    local PATH_TO_NET
    local PRE_1_4_0

    PATH_TO_NET="$(get_path_to_net)"

    #Checks stages dir for lowest protocol version
    pushd "$(get_path_to_stages)/stage-1/"
    PRE_1_4_0=$(find ./* -maxdepth 0 -type d | awk -F'/' '{ print $2 }' | tr -d '_' | sort | head -n 1)
    popd

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        # if the combined integers from the PROTOCOL_VERISON >= 140 ( 1_4_0 )
        if [ "$PRE_1_4_0" -ge "140" ]; then
            # Check new data.lmdb path under ..storage/<chain_name>/
            if [ -f "$PATH_TO_NET/nodes/node-$IDX/storage/$(get_chain_name)/data.lmdb" ]; then
                GLOBAL_STATE_OUTPUT=$("$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen \
                        system-contract-registry -d "$PATH_TO_NET"/nodes/node-"$IDX"/storage/"$(get_chain_name)" -s "$(nctl-view-chain-state-root-hash node=$IDX | awk '{ print $12 }' | tr '[:upper:]' '[:lower:]')")
            else
                GLOBAL_STATE_OUTPUT=$("$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen \
                        system-contract-registry -d "$PATH_TO_NET"/nodes/node-1/storage/"$(get_chain_name)" -s "$(nctl-view-chain-state-root-hash node=1 | awk '{ print $12 }' | tr '[:upper:]' '[:lower:]')")
            fi
        else
            if [ -f "$PATH_TO_NET/nodes/node-$IDX/storage/data.lmdb" ]; then
                GLOBAL_STATE_OUTPUT=$("$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen \
                        system-contract-registry -d "$PATH_TO_NET"/nodes/node-"$IDX"/storage)
            else
                GLOBAL_STATE_OUTPUT=$("$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen \
                        system-contract-registry -d "$PATH_TO_NET"/nodes/node-1/storage)
            fi
        fi

        echo "$GLOBAL_STATE_OUTPUT" > "$PATH_TO_NET/nodes/node-$IDX/config/$PROTOCOL_VERSION/global_state.toml"
    done
}
