#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Builds assets for staging.
# Arguments:
#   Stage ordinal identifier.
#   Stage source (local | remote | node_commit-client_commit).
#   Scenario protocol version.
#######################################
function _main()
{
    local STAGE_ID=${1}
    local STAGE_SOURCE=${2}
    local PROTOCOL_VERSION=${3}

    log "setting stage $STAGE_ID from $STAGE_SOURCE @ $PROTOCOL_VERSION -> STARTS"

    PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")/$PROTOCOL_VERSION"
    if [ ! -d "$PATH_TO_STAGE" ]; then
        mkdir -p "$PATH_TO_STAGE"
    fi

    if [ "$STAGE_SOURCE" == "local" ]; then
        source "$NCTL/sh/staging/set_from_local.sh" stage="$STAGE_ID" version="$PROTOCOL_VERSION"
    elif [ "$STAGE_SOURCE" == "remote" ]; then
        source "$NCTL/sh/staging/set_from_remote.sh" stage="$STAGE_ID" version="$PROTOCOL_VERSION"
    else
        source "$NCTL/sh/staging/set_from_commit.sh" stage="$STAGE_ID" version="$PROTOCOL_VERSION" commits="$STAGE_SOURCE"
    fi

    log "setting stage $STAGE_ID from $STAGE_SOURCE @ $PROTOCOL_VERSION -> COMPLETE"
}

#######################################
# Builds binaries.
# Arguments:
#   Stage node source code folder.
#   Stage client source code folder.
#######################################
function set_stage_binaries()
{
    local PATH_TO_NODE_SOURCE=${1}
    local PATH_TO_CLIENT_SOURCE=${2}

    pushd "$PATH_TO_NODE_SOURCE" || exit

    # Set node binary.
    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cargo build --package casper-node
    else
        cargo build --release --package casper-node
    fi

    # Set client-side wasm.
    make build-contract-rs/activate-bid
    make build-contract-rs/add-bid
    make build-contract-rs/delegate
    make build-contract-rs/named-purse-payment
    make build-contract-rs/transfer-to-account-u512
    make build-contract-rs/undelegate
    make build-contract-rs/withdraw-bid

    popd || exit

    # Set client binary.
    pushd "$PATH_TO_CLIENT_SOURCE" || exit

    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cargo build
    else
        cargo build --release
    fi

    popd || exit
}

#######################################
# Stages assets.
# Arguments:
#   Path to stage node source code folder.
#   Path to stage client source code folder.
#   Path to stage folder.
#######################################
function set_stage_files_from_repo()
{
    local PATH_TO_NODE_SOURCE=${1}
    local PATH_TO_CLIENT_SOURCE=${2}
    local PATH_TO_STAGE=${3}

    # Stage binaries.
    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cp "$PATH_TO_CLIENT_SOURCE/target/debug/casper-client" \
           "$PATH_TO_STAGE"
        cp "$PATH_TO_NODE_SOURCE/target/debug/casper-node" \
           "$PATH_TO_STAGE"
        cp "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/debug/casper-node-launcher" \
           "$PATH_TO_STAGE"
        cp "$PATH_TO_NODE_SOURCE/target/debug/casper-rpc-sidecar" \
           "$PATH_TO_STAGE"
    else
        cp "$PATH_TO_CLIENT_SOURCE/target/release/casper-client" \
           "$PATH_TO_STAGE"
        cp "$PATH_TO_NODE_SOURCE/target/release/casper-node" \
           "$PATH_TO_STAGE"
        cp "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/release/casper-node-launcher" \
           "$PATH_TO_STAGE"
        cp "$PATH_TO_NODE_SOURCE/target/release/casper-rpc-sidecar" \
           "$PATH_TO_STAGE"
    fi

    # Stage wasm.
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_AUCTION[@]}"
    do
        cp "$PATH_TO_NODE_SOURCE/target/wasm32-unknown-unknown/release/$CONTRACT" \
           "$PATH_TO_STAGE"
    done
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_SHARED[@]}"
    do
        cp "$PATH_TO_NODE_SOURCE/target/wasm32-unknown-unknown/release/$CONTRACT" \
           "$PATH_TO_STAGE"
    done
    for CONTRACT in "${NCTL_CONTRACTS_CLIENT_TRANSFERS[@]}"
    do
        cp "$PATH_TO_NODE_SOURCE/target/wasm32-unknown-unknown/release/$CONTRACT" \
           "$PATH_TO_STAGE"
    done

    # Stage chainspec.
    cp "$PATH_TO_NODE_SOURCE/resources/local/chainspec.toml.in" \
       "$PATH_TO_STAGE/chainspec.toml"

    # Stage node config.
    cp "$PATH_TO_NODE_SOURCE/resources/local/config.toml" \
       "$PATH_TO_STAGE"

    # Stage sidecar config.
    cp "${NCTL_CASPER_SIDECAR_HOME}/resources/example_configs/rpc_sidecar/sidecar.toml" \
       "$PATH_TO_STAGE"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset STAGE_ID
unset STAGE_SOURCE
unset PROTOCOL_VERSION

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        stage) STAGE_ID=${VALUE} ;;
        source) STAGE_SOURCE=${VALUE} ;;
        version) PROTOCOL_VERSION=${VALUE} ;;
        *)
    esac
done

_main "${STAGE_ID:-1}" \
      "${STAGE_SOURCE:-"local"}" \
      "${PROTOCOL_VERSION:-"1_0_0"}"
