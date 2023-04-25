#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# 1. Start nodes 1-5 in V1
# 2. Stage upgrade to V2 for node-6 only
# 3. Run node-6
# 4. Assert it logs error about the protocol version mismatch

# ----------------------------------------------------------------
# Imports.
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/node/svc_$NCTL_DAEMON_TYPE".sh
source "$NCTL/sh/scenarios/common/itst.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Main entry point.
function _main()
{
    local STAGE_ID=${1}

    if [ ! -d $(get_path_to_stage "$STAGE_ID") ]; then
        log "ERROR :: stage $STAGE_ID has not been built - cannot run scenario"
        exit 1
    fi

    _step_01 "$STAGE_ID"
    _step_02

    # Set initial protocol version for use later.
    INITIAL_PROTOCOL_VERSION=$(get_node_protocol_version 1)
    local ACTIVATION_POINT="$(($(get_chain_era) + NCTL_DEFAULT_ERA_ACTIVATION_OFFSET))"

    _step_03 "$STAGE_ID" "$ACTIVATION_POINT"
    _step_04 "6"
    _step_05 "6"
    _step_06 "6"
    _step_07
    _step_08
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}
    local PATH_TO_STAGE
    local PATH_TO_PROTO1

    PATH_TO_STAGE=$(get_path_to_stage "$STAGE_ID")
    pushd "$PATH_TO_STAGE"
    PATH_TO_PROTO1=$(ls -d */ | sort | head -n 1 | tr -d '/')
    popd

    log_step_upgrades 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" \
        stage="$STAGE_ID"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await for genesis
function _step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion"

    do_await_genesis_era_to_complete 'false'
}

# Step 03: Upgrade node-6 from stage.
function _step_03()
{
    local STAGE_ID=${1}
    local ACTIVATION_POINT=${2}

    log_step_upgrades 3 "upgrading node-6 from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/upgrade_from_stage_single_node.sh" \
        stage="$STAGE_ID" \
        verbose=false \
        node="6" \
        era="$ACTIVATION_POINT" \
        chainspec_path="$NCTL_CASPER_HOME/resources/local/chainspec.toml.in"
}

# Step 04: Join passive node.
function _step_04()
{
    local NODE_ID=${1}
    local TRUSTED_HASH

    log_step_upgrades 4 "joining passive node-$NODE_ID"

    log "... starting node-$NODE_ID"
    TRUSTED_HASH="$(get_chain_latest_block_hash)"
    if [ $(get_node_is_up "$NODE_ID") == false ]; then
        do_node_start "$NODE_ID" "$TRUSTED_HASH"
    fi
}

# Step 05: Assert joiner node is reporting protocol mismatch errors
function _step_05()
{
    local NODE_ID=${1}

    log_step_upgrades 5 "asserting error messages in node log"

    log "... allowing 15 seconds for the node to report errors"
    sleep 15

    local COUNT=$(cat "$NCTL"/assets/net-1/nodes/node-"$NODE_ID"/logs/stdout.log 2>/dev/null | grep 'peer is running incompatible version' | wc -l)
    if [ "$COUNT" -eq "0" ]; then
        log "ERROR: We got 0 error messages, but expected some"
        exit 1
    fi
}

# Step 06: Assert joiner node is not connected to any peers
function _step_06()
{
    local NODE_ID=${1}

    log_step_upgrades 6 "asserting node has no peers connected"

    local COUNT=$(get_node_connected_peer_count $NODE_ID)

    if [ "$COUNT" -ne "0" ]; then
        log "ERROR: We have $COUNT peers connected, but expected 0"
        exit 1
    fi
}

# Step 07: Run NCTL health checks
function _step_07()
{
    log_step_upgrades 7 "running health checks"
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='0' \
            equivocators='0' \
            doppels='0' \
            crashes=0 \
            restarts=0 \
            ejections=0
}

# Step 08: Terminate.
function _step_08()
{
    log_step_upgrades 8 "test successful - tidying up"

    source "$NCTL/sh/assets/teardown.sh"

    log_break
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset _STAGE_ID
unset INITIAL_PROTOCOL_VERSION

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        stage) _STAGE_ID=${VALUE} ;;
        *)
    esac
done

_main "${_STAGE_ID:-1}"
