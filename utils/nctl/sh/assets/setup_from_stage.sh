#!/usr/bin/env bash

#######################################
# Prepares assets for network start.
# Arguments:
#   Stage ordinal identifier.
#   Network ordinal identifier.
#   Network nodeset count.
#   Delay in seconds pripr to which genesis window will expire.
#   Path to custom accounts.toml.
#######################################

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/assets/setup_shared.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset PATH_TO_ACCOUNTS
unset GENESIS_DELAY_SECONDS
unset NET_ID
unset NODE_COUNT
unset STAGE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        accounts_path) PATH_TO_ACCOUNTS=${VALUE} ;;
        delay) GENESIS_DELAY_SECONDS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        nodes) NODE_COUNT=${VALUE} ;;
        stage) STAGE_ID=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}
GENESIS_DELAY_SECONDS=${GENESIS_DELAY_SECONDS:-30}
NODE_COUNT=${NODE_COUNT:-5}
PATH_TO_ACCOUNTS=${PATH_TO_ACCOUNTS:-""}
STAGE_ID="${STAGE_ID:-1}"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

function _main()
{
    log "setup of assets from stage ${1}: STARTS"

    local STAGE_ID=${1}
    local COUNT_NODES_AT_GENESIS=${2}
    local COUNT_NODES=$((COUNT_NODES_AT_GENESIS * 2))
    local GENESIS_DELAY=${3}
    local PATH_TO_ACCOUNTS=${4}
    local COUNT_USERS="$COUNT_NODES"
    local PATH_TO_NET
    local PATH_TO_STAGE

    PATH_TO_NET=$(get_path_to_net)
    PATH_TO_STAGE="$NCTL/stages/stage-$STAGE_ID/1_0_0"

    # Tear down previous.
    if [ -d "$PATH_TO_NET" ]; then
        source "$NCTL/sh/assets/teardown.sh" net="$NET_ID"
    fi
    mkdir -p "$PATH_TO_NET"

    # Setup new.
    setup_asset_directories "$COUNT_NODES" "$COUNT_USERS"

    setup_asset_binaries "1_0_0" \
                         "$COUNT_NODES" \
                         "$PATH_TO_STAGE/bin/casper-client" \
                         "$PATH_TO_STAGE/bin/casper-node" \
                         "$PATH_TO_STAGE/bin/casper-node-launcher" \
                         "$PATH_TO_STAGE/bin/wasm"

    setup_asset_keys "$COUNT_NODES" "$COUNT_USERS"

    setup_asset_daemon
    
    setup_asset_chainspec "$COUNT_NODES" \
                          "1.0.0" \
                          $(get_genesis_timestamp "$GENESIS_DELAY") \
                          "$PATH_TO_STAGE/resources/chainspec.toml"

    if [ "$PATH_TO_ACCOUNTS" = "" ]; then
        setup_asset_accounts "$COUNT_NODES" \
                             "$COUNT_NODES_AT_GENESIS" \
                             "$COUNT_USERS"
    else
        setup_asset_accounts_from_template "$COUNT_NODES" \
                                           "$COUNT_USERS" \
                                           "$PATH_TO_ACCOUNTS"
    fi

    setup_asset_node_configs "$COUNT_NODES" \
                             "1_0_0" \
                             "$PATH_TO_STAGE/resources/config.toml"

    log "setup of assets from stage ${1}: COMPLETE"
}

_main "$STAGE_ID" "$NODE_COUNT" "$GENESIS_DELAY_SECONDS" "$PATH_TO_ACCOUNTS"
