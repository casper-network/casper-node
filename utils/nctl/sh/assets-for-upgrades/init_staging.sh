#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SCENARIO_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        scenario) SCENARIO_ID=${VALUE} ;;
        *)
    esac
done

SCENARIO_ID=${SCENARIO_ID:-1}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Initialises scenario binaries.
# Arguments:
#   Scenario ordinal identifier.
#######################################
function _set_assets()
{
    local PATH_TO_SCENARIO=${1}
    local PATH_TO_SCENARIO_VERSION
    local SCENARIO_COMMIT_HASH
    local SCENARIO_VERSION
    local STAGING_TARGET
    local UPGRADE_STAGING_TARGET
    local IFS

    log "... setting assets"

    pushd "$PATH_TO_SCENARIO/src" || exit
    for UPGRADE_STAGING_TARGET in "${NCTL_UPGRADE_STAGING_TARGETS[@]}"
    do        
        # Destructure settings.
        IFS=':'
        read -ra STAGING_TARGET <<< "$UPGRADE_STAGING_TARGET"
        SCENARIO_COMMIT_HASH="${STAGING_TARGET[1]}"
        SCENARIO_VERSION="${STAGING_TARGET[0]}"
        PATH_TO_SCENARIO_VERSION="$PATH_TO_SCENARIO/staging/$SCENARIO_VERSION"
        log "... ... $SCENARIO_VERSION"

        # Set commit.
        git checkout "$SCENARIO_COMMIT_HASH" > /dev/null 2>&1

        # # Compile binaries.
        # if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        #     cargo build --package casper-node
        #     cargo build --package casper-client
        # else
        #     cargo build --release --package casper-node
        #     cargo build --release --package casper-client
        # fi
        # make build-contract-rs/add-bid
        # make build-contract-rs/delegate
        # make build-contract-rs/transfer-to-account-u512
        # make build-contract-rs/transfer-to-account-u512-stored
        # make build-contract-rs/undelegate
        # make build-contract-rs/withdraw-bid

        # # Copy binaries -> staging.
        # if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        #     cp "./target/debug/casper-node" "$PATH_TO_SCENARIO_VERSION/bin"
        #     cp "./target/debug/casper-client" "$PATH_TO_SCENARIO_VERSION/bin-client"
        # else
        #     cp "./target/release/casper-node" "$PATH_TO_SCENARIO_VERSION/bin"
        #     cp "./target/release/casper-client" "$PATH_TO_SCENARIO_VERSION/bin-client"
        # fi
        # for CONTRACT in "${NCTL_CONTRACTS_CLIENT[@]}"
        # do
        #     cp "./target/wasm32-unknown-unknown/release/$CONTRACT" "$PATH_TO_SCENARIO_VERSION/bin-client"
        # done 

        # Copy chainspec -> staging.
        cp "./resources/local/chainspec.toml.in" "$PATH_TO_SCENARIO_VERSION/chainspec"

        # Copy node config -> staging.
        cp "./resources/local/config.toml" "$PATH_TO_SCENARIO_VERSION/config"
    done  
    popd || exit
}

#######################################
# Initialises file system with scenario directories.
# Arguments:
#   Scenario ordinal identifier.
#######################################
function _set_fs()
{
    local PATH_TO_SCENARIO=${1}
    local PATH_TO_SCENARIO_VERSION
    local SCENARIO_VERSION
    local UPGRADE_STAGING_TARGET
    local IFS

    log "... setting directories"

    # Set source code directory.
    if [ ! -d "$PATH_TO_SCENARIO/src" ]; then
        mkdir "$PATH_TO_SCENARIO/src"
        git clone "https://github.com/CasperLabs/casper-node.git" "$PATH_TO_SCENARIO/src" > /dev/null 2>&1
    fi

    # Set staging directory.
    if [ ! -d "$PATH_TO_SCENARIO/staging" ]; then
        mkdir "$PATH_TO_SCENARIO/staging"
    fi

    # Set protocol version staging directories.
    for UPGRADE_STAGING_TARGET in "${NCTL_UPGRADE_STAGING_TARGETS[@]}"
    do
        # Destructure settings.
        IFS=':'
        read -ra STAGING_TARGET <<< "$UPGRADE_STAGING_TARGET"
        SCENARIO_VERSION="${STAGING_TARGET[0]}"
        PATH_TO_SCENARIO_VERSION="$PATH_TO_SCENARIO/staging/$SCENARIO_VERSION"
    
        # Set directories.
        if [ ! -d "$PATH_TO_SCENARIO_VERSION" ]; then
            mkdir "$PATH_TO_SCENARIO_VERSION"
            mkdir "$PATH_TO_SCENARIO_VERSION/bin"
            mkdir "$PATH_TO_SCENARIO_VERSION/bin-client"
            mkdir "$PATH_TO_SCENARIO_VERSION/chainspec"
            mkdir "$PATH_TO_SCENARIO_VERSION/config"
        fi
    done
}

#######################################
# Prepares assets for upgrade scenarios.
# Arguments:
#   Scenario ordinal identifier.
#######################################
function _main()
{
    local SCENARIO_ID=${1}
    local PATH_TO_SCENARIO="$NCTL/assets-for-upgrades/scenario-$SCENARIO_ID"

    log "upgrade scenario asset setup step 1: STARTS"

    source "$PATH_TO_SCENARIO/settings.sh"
    _set_fs "$PATH_TO_SCENARIO"
    _set_assets "$PATH_TO_SCENARIO"    

    log "upgrade scenario asset setup step 1: COMPLETE"
}

_main "$SCENARIO_ID"