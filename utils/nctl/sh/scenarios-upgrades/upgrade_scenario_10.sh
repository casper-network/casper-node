#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# 1. Start v1 running at ProtocolVersion 1_4_5 commit.
# 2. Waits for genesis era to complete.
# 3. Query auction-info at block height 1.
# 4. Run through an upgrade
# 5. Query auction-info at block height 1 and compare with previous result.
# 6. Successful test cleanup.

# ----------------------------------------------------------------
# Imports.
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/node/svc_$NCTL_DAEMON_TYPE".sh

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------


# Main entry point.
function _main()
{
      local STAGE_ID=${1}
      local HISTORIC_AUCTION_INFO

      if [ ! -d "$(get_path_to_stage "$STAGE_ID")" ]; then
          log "ERROR :: stage $STAGE_ID has not been built - cannot run scenario"
          exit 1
      fi

      _step_01 "$STAGE_ID"
      _step_02
      _step_03
      _step_04
      _step_05
      _step_06
}

# Step 01: Start network from pre-built stage.
function _step_01()
{
    local STAGE_ID=${1}

    log_step_upgrades 1 "starting network from stage ($STAGE_ID)"

    source "$NCTL/sh/assets/setup_from_stage.sh" \
            stage="$STAGE_ID"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await era-id >= 1.
function _step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion"
    await_n_eras '1' 'true' '5.0'
}

function assert_present_era_infos() {
    local NODE_ID=${1}
    local BLOCK=$(nctl-view-chain-block)
    local CURRENT_ERA_ID=$(jq -r '.header.era_id' <<< "$BLOCK")
    local SEQ_END=$(($CURRENT_ERA_ID-1))
    local STATE_ROOT_HASH=$(jq -r '.header.state_root_hash' <<< "$BLOCK")
    log "state root hash $STATE_ROOT_HASH"
    local total=0
    for i in $(seq 0 $SEQ_END);
    do
        OUTPUT=$($(get_path_to_client) \
            query-global-state \
            --node-address "$(get_node_address_rpc "$NODE_ID")" \
            --state-root-hash "$STATE_ROOT_HASH" \
            --key era-$i)
        RESULT="$?"
        if [ $RESULT -eq 0 ]; then
            local total=$(($total+1))
        else
            log Unable to query era $i
            log $OUTPUT
            exit 1
        fi
    done
    if [ ! $total -eq $CURRENT_ERA_ID ]; then
        log "Expected $CURRENT_ERA_ID era infos but queried $total"
        exit 1
    fi
}

# Step 03: Query auction info
function _step_03() {
    local NODE_ID=${1}
    log_step_upgrades 3 "querying for latest era infos"

    assert_present_era_infos
}

# Step 04: Upgrade network from stage.
function _step_04()
{
    local STAGE_ID=${1}

    log_step_upgrades 4 "upgrading network from stage ($STAGE_ID)"

    log "... setting upgrade assets"
    source "$NCTL/sh/assets/upgrade_from_stage.sh" \
        stage="$STAGE_ID" \
        verbose=false \
        chainspec_path="$NCTL/overrides/upgrade_scenario_10.post.chainspec.toml.in"

    log "... awaiting era"

    # previous protocol had era id of 1, and new version apparently activates at era 3
    await_n_eras '2' 'true' '5.0'
    log "... awaiting multiple blocks"
    await_n_blocks 5
}

function _step_05() {
    log_step_upgrades 5 "... asserting that there are no era infos present in the global state"
    local NODE_ID=${1}
    local BLOCK=$(nctl-view-chain-block)

    local CURRENT_ERA_ID=$(jq -r '.header.era_id' <<< "$BLOCK")
    local SEQ=$(($CURRENT_ERA_ID-1))
    local STATE_ROOT_HASH=$(jq -r '.header.state_root_hash' <<< "$BLOCK")
    log "state root hash $STATE_ROOT_HASH"
    local TOTAL=0
    set +e
    for i in $(seq 0 $SEQ);
    do
        OUTPUT=$($(get_path_to_client) \
            query-global-state \
            --node-address "$(get_node_address_rpc "$NODE_ID")" \
            --state-root-hash "$STATE_ROOT_HASH" \
            --key era-$i)
        RESULT="$?"
        if [ $RESULT -eq 0 ]; then
            TOTAL=$(($TOTAL+1))
        else
            log "Querying for era $i output"
            log "$OUTPUT"
        fi
    done
    set -e
    log "found $TOTAL era infos for a total of $CURRENT_ERA_ID eras"
    if [ ! $TOTAL -eq 0 ]; then
        log "Should have 0 era infos total..."
        exit 1
    else
        log "Success!"
    fi
}

# Step 06: Terminate.
function _step_06()
{
    log_step_upgrades 6 "test successful - tidying up"

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
