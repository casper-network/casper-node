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
    local PATH_TO_SIDECAR=${6}
    local PATH_TO_WASM=${7}

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

        if [ -f "$PATH_TO_SIDECAR" ]; then
            cp "$PATH_TO_SIDECAR" "$PATH_TO_BIN/$PROTOCOL_VERSION"
        fi
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
        )
    else
        SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_CHAINSPEC');"
            "cfg['protocol']['activation_point']=$ACTIVATION_POINT;"
            "cfg['protocol']['version']='$PROTOCOL_VERSION';"
            "cfg['network']['name']='$(get_chain_name)';"
        )
    fi

    if [[ "$PATH_TO_CHAINSPEC_TEMPLATE" == *"resources/local/chainspec.toml.in"* ]] || \
       [[ "$PATH_TO_CHAINSPEC_TEMPLATE" == *"stages"* ]]; then
        SCRIPT+=("cfg['core']['validator_slots']=$COUNT_NODES;")
    fi

    SCRIPT+=("toml.dump(cfg, open('$PATH_TO_CHAINSPEC', 'w'));")

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
# Sets node configuration files.
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
    local PATH_TO_SIDECAR_TEMPLATE=${4}
    local IS_GENESIS=${5}

    local IDX
    local PATH_TO_NET
    local PATH_TO_CONFIG
    local PATH_TO_CONFIG_FILE
    local PATH_TO_SIDECAR_CONFIG_FILE
    local SCRIPT
    local SPECULATIVE_EXEC_ADDR

    PATH_TO_NET="$(get_path_to_net)"

    for IDX in $(seq 1 "$COUNT_NODES")
    do
        # Set paths to node's config.
        PATH_TO_CONFIG="$(get_path_to_node "$IDX")/config/$PROTOCOL_VERSION"
        PATH_TO_CONFIG_FILE="$PATH_TO_CONFIG/config.toml"
        PATH_TO_SIDECAR_CONFIG_FILE="$PATH_TO_CONFIG/sidecar.toml"

        # Set node configuration.
        if [ "$IS_GENESIS" == true ]; then
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
            "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$IDX")';"
        )

        BINARY_PORT_SERVER_ADDR=$(grep 'binary_port_server' $PATH_TO_CONFIG_FILE || true)
        if [ ! -z "$BINARY_PORT_SERVER_ADDR" ]; then
            SCRIPT+=(
                "cfg['binary_port_server']['address']='0.0.0.0:$(get_node_port_binary "$IDX")';"
            )
        fi

        RPC_SERVER_ADDR=$(grep 'rpc_server' $PATH_TO_CONFIG_FILE || true)
        if [ ! -z "$RPC_SERVER_ADDR" ]; then
            SCRIPT+=(
                "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$IDX")';"
            )
        fi

        SPECULATIVE_EXEC_ADDR=$(grep 'speculative_exec_server' $PATH_TO_CONFIG_FILE || true)
        if [ ! -z "$SPECULATIVE_EXEC_ADDR" ]; then
            SCRIPT+=(
                "cfg['speculative_exec_server']['address']='0.0.0.0:$(get_node_port_speculative_exec "$IDX")';"
            )
        fi

        SCRIPT+=(
            "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
        )

        if [ -f "$PATH_TO_SIDECAR_TEMPLATE" ]; then
            # Prepare the sidecar config file.
            cp "$PATH_TO_SIDECAR_TEMPLATE" "$PATH_TO_SIDECAR_CONFIG_FILE"

             SCRIPT+=(
                "cfg=toml.load('$PATH_TO_SIDECAR_CONFIG_FILE');"
                "cfg['rpc_server']['address']='0.0.0.0:$(get_node_port_rpc "$IDX")';"
                "cfg['speculative_exec_server']['address']='0.0.0.0:$(get_node_port_speculative_exec "$IDX")';"
                "cfg['node_client']['address']='0.0.0.0:$(get_node_port_binary "$IDX")';"
                "toml.dump(cfg, open('$PATH_TO_SIDECAR_CONFIG_FILE', 'w'));"
            )
        fi

        python3 -c "${SCRIPT[*]}"
    done
}

function setup_asset_global_state_toml_for_node() {
    local IDX=${1}
    local TARGET_PROTOCOL_VERSION=${2}
    local PATH_TO_NET="$(get_path_to_net)"
    local STORAGE_PATH="$PATH_TO_NET/nodes/node-$IDX/storage"

    if [ -f "$STORAGE_PATH/$(get_chain_name)/data.lmdb" ]; then
        GLOBAL_STATE_OUTPUT=$("$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen \
                migrate-into-system-contract-registry -d "$STORAGE_PATH"/"$(get_chain_name)")
    else
        GLOBAL_STATE_OUTPUT=$("$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen \
                migrate-into-system-contract-registry -d "$STORAGE_PATH")
    fi

    echo "$GLOBAL_STATE_VALIDATOR_OUTPUT" > "$PATH_TO_NET/nodes/node-$IDX/config/$TARGET_PROTOCOL_VERSION/global_state.toml"
    echo "$GLOBAL_STATE_OUTPUT" >> "$PATH_TO_NET/nodes/node-$IDX/config/$TARGET_PROTOCOL_VERSION/global_state.toml"
}

function setup_asset_global_state_toml() {
    log "... setting node global_state.toml"

    local COUNT_NODES=${1}
    local COUNT_NODES_AT_GENESIS=$((COUNT_NODES / 2))
    local TARGET_PROTOCOL_VERSION
    local VERSION_14_BOUNDARY

    local SEMVER_GLOBAL_STATE_UPDATE_TOOL
    local SEMVER_STAGE_1_PRE_VERSION

    read -ra SEMVER_GLOBAL_STATE_UPDATE_TOOL <<< "$("$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen --version | grep -oE '[^ ]+$' | tr '.' ' ')"

    # Check version of the global state update tool. 0.3 marks the transition to 1.5 fast sync node and supports the "validators" command.
    if [ "${SEMVER_GLOBAL_STATE_UPDATE_TOOL[1]}" -lt "3" ]; then
        log "ERROR :: Global State Update Generator must be version 0.3 or greater (found version ${SEMVER_GLOBAL_STATE_UPDATE_TOOL[0]}.${SEMVER_GLOBAL_STATE_UPDATE_TOOL[1]}.${SEMVER_GLOBAL_STATE_UPDATE_TOOL[2]})"
        exit 1
    fi

    pushd "$(get_path_to_stages)/stage-1/"
    read -ra SEMVER_STAGE_1_PRE_VERSION <<< $(find ./* -maxdepth 0 -type d | awk -F'/' '{ print $2 }' |  sort | head -n 1 | tr '_' ' ')
    popd
    pushd "$(get_path_to_stages)/stage-1/"
    read -ra SEMVER_STAGE_1_POST_VERSION <<< $(find ./* -maxdepth 0 -type d | awk -F'/' '{ print $2 }' |  sort | tail -n 1 | tr '_' ' ')
    local TARGET_PROTOCOL_VERSION=$(find ./* -maxdepth 0 -type d | awk -F'/' '{ print $2 }' | sort | tail -n 1)
    popd

    log "... processing upgrade from ${SEMVER_STAGE_1_PRE_VERSION[0]}.${SEMVER_STAGE_1_PRE_VERSION[1]}.${SEMVER_STAGE_1_PRE_VERSION[2]} to ${SEMVER_STAGE_1_POST_VERSION[0]}.${SEMVER_STAGE_1_POST_VERSION[1]}.${SEMVER_STAGE_1_POST_VERSION[2]}"

    if [ "${SEMVER_STAGE_1_PRE_VERSION[0]}" -le "1" ] && \
       [ "${SEMVER_STAGE_1_PRE_VERSION[1]}" -lt "4" ] && \
       [ "${SEMVER_STAGE_1_POST_VERSION[0]}" -ge "1" ] && \
       [ "${SEMVER_STAGE_1_POST_VERSION[1]}" -ge "4" ]; then
        log "... upgrading across the 1.4.0 boundary, generating 'global_state.toml' file"
        VERSION_14_BOUNDARY=1
    else
        log "... not upgrading across the 1.4.0 boundary, no 'global_state.toml' file needed"
        VERSION_14_BOUNDARY=0
    fi

    if [ "$VERSION_14_BOUNDARY" -eq "1" ]; then
        for IDX in $(seq 1 "$COUNT_NODES_AT_GENESIS")
        do
            setup_asset_global_state_toml_for_node $IDX $TARGET_PROTOCOL_VERSION
        done
    fi
}
