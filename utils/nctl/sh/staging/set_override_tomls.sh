#!/usr/bin/env bash

#######################################
# Downloads remote assets for subsequent staging.
# Arguments:
#   Protocol version to be downloaded.
#######################################

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

function _main()
{
    log "Generating overridden toml files..."
    local SCENARIOS_DIR
    local SCENARIOS_CONFIGS_DIR
    local SCENARIOS_CHAINSPECS_DIR
    local SCENARIOS_ACCOUNTS_DIR
    local CONFIGS
    local CHAINSPECS
    local ACCOUNTS
    local LOCAL_CONFIGS
    local LOCAL_CHAINSPECS
    local LOCAL_ACCOUNTS
    local STAGE_DIR
    local CI_JSON_CONFIG_FILE
    local PROTO_1

    SCENARIOS_DIR="$NCTL/sh/scenarios"
    SCENARIOS_CONFIGS_DIR="$SCENARIOS_DIR/configs"
    SCENARIOS_CHAINSPECS_DIR="$SCENARIOS_DIR/chainspecs"
    SCENARIOS_ACCOUNTS_DIR="$SCENARIOS_DIR/accounts_toml"

    LOCAL_CONFIG="$NCTL_CASPER_HOME/resources/local/config.toml"
    LOCAL_CHAINSPEC="$NCTL_CASPER_HOME/resources/local/chainspec.toml.in"
    LOCAL_ACCOUNT="$NCTL_CASPER_HOME/resources/local/accounts.toml"

    STAGE_DIR="$NCTL/overrides"
    CI_JSON_CONFIG_FILE="$NCTL/ci/ci.json"
    PROTO_1=$(jq -r '.nctl_upgrade_tests."protocol_1"' "$CI_JSON_CONFIG_FILE")
    PROTO_DIR="$NCTL/remotes/$PROTO_1"

    mkdir -p "$STAGE_DIR"

    # check if config overrides dir exist
    if [ -d "$SCENARIOS_CONFIGS_DIR" ]; then
        log "... config overrides directory found"
        pushd "$SCENARIOS_CONFIGS_DIR"

        if [ "$NCTL_UPGRADE_TEST" = false ]; then
            CONFIGS=($(ls "$SCENARIOS_CONFIGS_DIR" | grep -v "upgrade_scenario" | awk -F'.' '{print $1}'))
        else
            CONFIGS=($(ls "$SCENARIOS_CONFIGS_DIR" | grep "upgrade_scenario" | awk -F'.' '{print $1}'))
        fi

        for i in "${CONFIGS[@]}"; do
            if [[ "$i" == *"upgrade_scenario"* ]]; then
                # Pre
                call_config_gen "$i.config.toml.override" "$PROTO_DIR/config.toml" "$STAGE_DIR/$i.pre.config.toml"
                # Post
                call_config_gen "$i.config.toml.override" "$LOCAL_CONFIG" "$STAGE_DIR/$i.post.config.toml"
            else
                # Itsts
                call_config_gen "$i.config.toml.override" "$LOCAL_CONFIG" "$STAGE_DIR/$i.config.toml"
            fi
        done
        popd
    fi

    # check if chainspec overrides dir exist
    if [ -d "$SCENARIOS_CHAINSPECS_DIR" ]; then
        log "... chainspec overrides directory found"
        pushd "$SCENARIOS_CHAINSPECS_DIR"

        if [ "$NCTL_UPGRADE_TEST" = false ]; then
            CHAINSPECS=($(ls "$SCENARIOS_CHAINSPECS_DIR" | grep -v "upgrade_scenario" | awk -F'.' '{print $1}'))
        else
            CHAINSPECS=($(ls "$SCENARIOS_CHAINSPECS_DIR" | grep "upgrade_scenario" | awk -F'.' '{print $1}'))
        fi

        for i in "${CHAINSPECS[@]}"; do
            if [[ "$i" == *"upgrade_scenario"* ]]; then
                # Pre
                call_config_gen "$i.chainspec.toml.override" "$PROTO_DIR/chainspec.toml.in" "$STAGE_DIR/$i.pre.chainspec.toml.in"
                # Post
                call_config_gen "$i.chainspec.toml.override" "$LOCAL_CHAINSPEC" "$STAGE_DIR/$i.post.chainspec.toml.in"
            else
                # Itsts
                call_config_gen "$i.chainspec.toml.override" "$LOCAL_CHAINSPEC" "$STAGE_DIR/$i.chainspec.toml.in"
            fi
        done
        popd
    fi

    # check if accounts overrides dir exist
    if [ -d "$SCENARIOS_ACCOUNTS_DIR" ]; then
        log "... account overrides directory found"
        pushd "$SCENARIOS_ACCOUNTS_DIR"

        if [ "$NCTL_UPGRADE_TEST" = false ]; then
            ACCOUNTS=($(ls "$SCENARIOS_ACCOUNTS_DIR" | grep -v "upgrade_scenario" | awk -F'.' '{print $1}'))
        else
            ACCOUNTS=($(ls "$SCENARIOS_ACCOUNTS_DIR" | grep "upgrade_scenario" | awk -F'.' '{print $1}'))
        fi

        for i in "${ACCOUNTS[@]}"; do
            if [[ "$i" == *"upgrade_scenario"* ]]; then
                # Pre
                call_config_gen "$i.accounts.toml.override" "$PROTO_DIR/accounts.toml" "$STAGE_DIR/$i.pre.accounts.toml"
                # Post
                call_config_gen "$i.accounts.toml.override" "$LOCAL_ACCOUNT" "$STAGE_DIR/$i.post.accounts.toml"
            else
                # Itsts
                call_config_gen "$i.accounts.toml.override" "$LOCAL_ACCOUNT" "$STAGE_DIR/$i.accounts.toml"
            fi
        done
        popd
    fi

}

function call_config_gen() {
    local OVERRIDE_SCRIPT
    local OVERRIDE_FILE=${1}
    local TOML_FILE=${2}
    local OUTPUT_FILE=${3}

    OVERRIDE_SCRIPT="$NCTL/scripts/config_gen.py"

    # itst06_private_chain and itst07_private_chain tests need the `--no_skip` flag in order
    # to pull the administrators section from the override toml. Without `--no_skip` the override
    # mechanism will skip any fields that are missing from the base toml file.
    if [ "$OVERRIDE_FILE" == "itst06_private_chain.accounts.toml.override" ] || [ "$OVERRIDE_FILE" == "itst07_private_chain.accounts.toml.override" ]; then
        "$OVERRIDE_SCRIPT" --override_file "$OVERRIDE_FILE" \
            --toml_file "$TOML_FILE" \
            --output_file "$OUTPUT_FILE" \
            --no_skip
    else
        "$OVERRIDE_SCRIPT" --override_file "$OVERRIDE_FILE" \
            --toml_file "$TOML_FILE" \
            --output_file "$OUTPUT_FILE"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset NCTL_UPGRADE_TEST

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        upgrade_test) NCTL_UPGRADE_TEST=${VALUE} ;;
        *)
    esac
done

NCTL_UPGRADE_TEST=${NCTL_UPGRADE_TEST:-false}

_main "$NCTL_UPGRADE_TEST"
