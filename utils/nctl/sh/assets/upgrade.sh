#!/usr/bin/env bash

#######################################
# Imports
#######################################

source "$NCTL"/sh/utils/main.sh

#######################################
# Upgrades node in the network
# Arguments:
#   Protocol version
#   Era at which new version should be upgraded
#   ID of the node to upgrade
#######################################
function _upgrade_node() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local NODE_ID=${3}
    local CONFIG_PATH=${4}
    local PATH_TO_NET=$(get_path_to_net)
    local PATH_TO_CONFIG_FILE
    local SPECULATIVE_EXEC_ADDR
    local PATH_TO_CHAINSPEC_FILE=${5:-"$PATH_TO_NET/chainspec/chainspec.toml"}

    local PATH_TO_NODE
    local CHAIN_NAME

    # Set chainspec file.
    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"
    PATH_TO_UPGRADED_CHAINSPEC_FILE="$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/chainspec.toml
    cp "$PATH_TO_CHAINSPEC_FILE" "$PATH_TO_UPGRADED_CHAINSPEC_FILE"

    # Really make sure the chain name stays the same :)
    CHAIN_NAME=$(get_chain_name)

    # Write chainspec contents.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CHAINSPEC_FILE');"
        "cfg['protocol']['version']='$PROTOCOL_VERSION'.replace('_', '.');"
        "cfg['protocol']['activation_point']=$ACTIVATE_ERA;"
        "cfg['network']['name']='$CHAIN_NAME';"
        "toml.dump(cfg, open('$PATH_TO_UPGRADED_CHAINSPEC_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    # Copy casper-node binary.
    PATH_TO_NODE=$(get_path_to_node "$NODE_ID")
    mkdir -p "$PATH_TO_NODE"/bin/"$PROTOCOL_VERSION"

    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cp "$NCTL_CASPER_HOME"/target/debug/casper-node "$PATH_TO_NODE"/bin/"$PROTOCOL_VERSION"/
    else
        cp "$NCTL_CASPER_HOME"/target/release/casper-node "$PATH_TO_NODE"/bin/"$PROTOCOL_VERSION"/
    fi

    # Copy chainspec.
    mkdir -p "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/
    cp "$PATH_TO_UPGRADED_CHAINSPEC_FILE" "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/

    # Copy config file.
    if [ -z "$CONFIG_PATH" ]; then
        cp $(get_path_to_node_config_file "$NODE_ID") "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/
    else
        # Set paths to node's config.
        PATH_TO_CONFIG_FILE="$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/config.toml

        cp "$CONFIG_PATH" "$PATH_TO_CONFIG_FILE"

        SPECULATIVE_EXEC_ADDR=$(grep 'speculative_exec_server' $PATH_TO_CONFIG_FILE || true)

        local SCRIPT=(
            "import toml;"
            "cfg=toml.load('$PATH_TO_NODE/config/$PROTOCOL_VERSION/config.toml');"
            "cfg['consensus']['secret_key_path']='../../keys/secret_key.pem';"
            "cfg['logging']['format']='$NCTL_NODE_LOG_FORMAT';"
            "cfg['network']['bind_address']='$(get_network_bind_address "$NODE_ID")';"
            "cfg['network']['known_addresses']=[$(get_network_known_addresses "$NODE_ID")];"
            "cfg['storage']['path']='../../storage';"
            "cfg['rest_server']['address']='0.0.0.0:$(get_node_port_rest "$NODE_ID")';"
            "cfg['binary_port_server']['address']='0.0.0.0:$(get_node_port_binary "$NODE_ID")';"
            "cfg['event_stream_server']['address']='0.0.0.0:$(get_node_port_sse "$NODE_ID")';"
        )
        
        SCRIPT+=(
            "toml.dump(cfg, open('$PATH_TO_CONFIG_FILE', 'w'));"
        )

        python3 -c "${SCRIPT[*]}"
    fi

    # Clean up.
    rm "$PATH_TO_UPGRADED_CHAINSPEC_FILE"
}

function _validators_state_update_config()
{
    local NODE_COUNT=$1
    local NODE_ID
    local PUBKEY
    TEMPFILE=$(mktemp)

    echo 'only_listed_validators = true' >> $TEMPFILE

    for NODE_ID in $(seq $((NODE_COUNT + 1)) $((NODE_COUNT * 2))); do
        PUBKEY=`cat $(get_path_to_node $NODE_ID)/keys/public_key_hex`
        cat << EOF >> $TEMPFILE
[[accounts]]
public_key = "$PUBKEY"

[accounts.validator]
bonded_amount = "$(($NODE_ID + 1000000000000000))"

EOF
    done

    echo $TEMPFILE
}

#######################################
# Generates a global state update for an emergency upgrade
# Arguments:
#   Protocol version
#   Pre-upgrade global state root hash
#   ID of the node to use as the source of the pre-upgrade global state
#   Number of the nodes in the network
#######################################
function _generate_global_state_update() {
    local PROTOCOL_VERSION=${1}
    local STATE_HASH=${2}
    local STATE_SOURCE=${3:-1}
    local NODE_COUNT=${4:-5}
    #NOTE ${5} used for params, update accordingly if needed

    local PATH_TO_NET=$(get_path_to_net)

    if [ -f "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml ]; then
        # global state update file exists, no need to generate it again
        return
    fi

    local STATE_SOURCE_PATH=$(get_path_to_node $STATE_SOURCE)

    # Create parameters to the global state update generator.
    # First, we supply the path to the directory of the node whose global state we'll use
    # and the trusted hash then we Add the parameters that define the new validators.
    # We're using the reserve validators, from NODE_COUNT+1 to NODE_COUNT*2.
    local PARAMS=${5:-"generic -d ${STATE_SOURCE_PATH}/storage/$(get_chain_name) -s ${STATE_HASH} $(_validators_state_update_config $NODE_COUNT)"}

    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"

    # Create the global state update file.
    "$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen $PARAMS \
        > "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml

    rm -f $TEMPFILE
}

#######################################
# Performs an emergency upgrade on a node in the network
# Arguments:
#   Protocol version
#   Era at which new version should be upgraded
#   ID of the node to upgrade
#   Pre-upgrade global state root hash
#   ID of a node to be used as the source of the pre-upgrade global state
#   The number of nodes in the network
#######################################
function _emergency_upgrade_node() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local NODE_ID=${3}
    local STATE_HASH=${4}
    local STATE_SOURCE=${5:-1}
    local NODE_COUNT=${6:-5}
    local CONFIG_PATH=${7:-""}
    local CHAINSPEC_PATH=${8:-""}
    local GENERATE_GS_UPDATE=${9:-"true"}

    _upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID" "$CONFIG_PATH" "$CHAINSPEC_PATH"

    local PATH_TO_NODE=$(get_path_to_node $NODE_ID)

    # Specify hard reset in the chainspec.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_NODE/config/$PROTOCOL_VERSION/chainspec.toml');"
        "cfg['protocol']['hard_reset']=True;"
        "toml.dump(cfg, open('$PATH_TO_NODE/config/$PROTOCOL_VERSION/chainspec.toml', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    local PATH_TO_NET=$(get_path_to_net)

    if [ "$GENERATE_GS_UPDATE" == 'true' ]; then
        _generate_global_state_update "$PROTOCOL_VERSION" "$STATE_HASH" "$STATE_SOURCE" "$NODE_COUNT"
    fi

    cp "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml \
        "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/global_state.toml

    # remove stored state of the launcher - this will make the launcher start from the highest
    # available version instead of from the previously executed one
    if [ -e "$PATH_TO_NODE"/config/casper-node-launcher-state.toml ]; then
        rm "$PATH_TO_NODE"/config/casper-node-launcher-state.toml
    fi
}

#######################################
# Generates a global state update for an emergency upgrade performing a balance adjustment
# Arguments:
#   Protocol version
#   Pre-upgrade global state root hash
#   ID of the node to use as the source of the pre-upgrade global state
#   Account hash of the source account
#   Account hash of the target account
#   The amount of motes to be transferred
#   The public key of the proposer
#######################################
function _generate_global_state_update_balances() {
    local PROTOCOL_VERSION=${1}
    local STATE_HASH=${2}
    local STATE_SOURCE=${3:-1}
    local SRC_ACC=${4}
    local TARGET_ACC=${5}
    local AMOUNT=${6}

    local PATH_TO_NET=$(get_path_to_net)

    if [ -f "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml ]; then
        # global state update file exists, no need to generate it again
        return
    fi

    local STATE_SOURCE_PATH=$(get_path_to_node $STATE_SOURCE)

    local TEMPFILE=$(mktemp)
    cat << EOF >> $TEMPFILE
[[transfers]]
from = "${SRC_ACC}"
to = "${TARGET_ACC}"
amount = "${AMOUNT}"

EOF

    # Create parameters to the global state update generator.
    # First, we supply the path to the directory of the node whose global state we'll use
    # and the trusted hash.
    local PARAMS
    PARAMS="generic -d ${STATE_SOURCE_PATH}/storage/$(get_chain_name) -s ${STATE_HASH} $TEMPFILE"

    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"

    # Create the global state update file.
    "$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen $PARAMS \
        > "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml

    rm -f $TEMPFILE
}

#######################################
# Performs an emergency upgrade on a node in the network
# Arguments:
#   Protocol version
#   Era at which new version should be upgraded
#   ID of the node to upgrade
#   Pre-upgrade global state root hash
#   ID of a node to be used as the source of the pre-upgrade global state
#   Account hash of the source account
#   Account hash of the target account
#   The amount of motes to be transferred
#   The public key of the proposer
#######################################
function _emergency_upgrade_node_balances() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local NODE_ID=${3}
    local STATE_HASH=${4}
    local STATE_SOURCE=${5:-1}
    local SRC_ACC=${6}
    local TARGET_ACC=${7}
    local AMOUNT=${8}
    local PROPOSER=${9}

    _upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID"

    local PATH_TO_NODE=$(get_path_to_node $NODE_ID)

    # Specify hard reset in the chainspec.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_NODE/config/$PROTOCOL_VERSION/chainspec.toml');"
        "cfg['protocol']['hard_reset']=True;"
        "toml.dump(cfg, open('$PATH_TO_NODE/config/$PROTOCOL_VERSION/chainspec.toml', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    local PATH_TO_NET=$(get_path_to_net)

    _generate_global_state_update_balances "$PROTOCOL_VERSION" "$STATE_HASH" "$STATE_SOURCE" "$SRC_ACC" "$TARGET_ACC" "$AMOUNT" "$PROPOSER"

    cp "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml \
        "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/global_state.toml

    # remove stored state of the launcher - this will make the launcher start from the highest
    # available version instead of from the previously executed one
    if [ -e "$PATH_TO_NODE"/config/casper-node-launcher-state.toml ]; then
        rm "$PATH_TO_NODE"/config/casper-node-launcher-state.toml
    fi
}
