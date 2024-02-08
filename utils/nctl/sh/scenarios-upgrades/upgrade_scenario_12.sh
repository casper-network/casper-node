#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# 1. Start v1 running at ProtocolVersion 1_3_0 commit.
# 2. Waits for genesis era to complete.
# 3. Query auction-info at block height 1.
# 4. Run through an upgrade
# 5. Query auction-info at block height 1 and compare with previous result.
# 6. Run Health Checks
# 7. Successful test cleanup.

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
      _step_07
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
            stage="$STAGE_ID" \
            chainspec_path="$NCTL/remotes/1.3.0/chainspec.toml.in"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Await for genesis
function _step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion"

    do_await_genesis_era_to_complete 'false'
}

# Step 03: Query auction info
function _step_03() {
    log_step_upgrades 3 "querying for historical information"

    HISTORIC_AUCTION_INFO="$(get_auction_state_at_block_1)"
}

# Step 04: Upgrade network from stage.
function _step_04()
{
    local STAGE_ID=${1}

    log_step_upgrades 4 "upgrading network from stage ($STAGE_ID)"

    log "... setting upgrade assets"
    source "$NCTL/sh/assets/upgrade_from_stage.sh" \
        stage="$STAGE_ID" \
        chainspec_path="$NCTL_CASPER_HOME/resources/local/chainspec.toml.in" \
        verbose=false

    log "... awaiting 2 eras + 1 block"
    nctl-await-n-eras offset='2' sleep_interval='5.0' timeout='180'
    await_n_blocks 1
}

function _step_05() {
    log_step_upgrades 5 "querying for historical information after upgrade"

    local AUCTION_INFO="$(get_auction_state_at_block_1)"

    # The format of `bids` array has changed at node 2.0.  Compare the era validators only.
    local HISTORIC_VALIDATORS="$(echo "$HISTORIC_AUCTION_INFO" | jq .era_validators)"
    local VALIDATORS="$(echo "$AUCTION_INFO" | jq .era_validators)"

    if [ "$VALIDATORS" != "$HISTORIC_VALIDATORS" ]; then
      log "Error auction info does not match"
      echo "$VALIDATORS"
      echo "$HISTORIC_VALIDATORS"
      exit 1
    fi

}

# Step 06: Run NCTL health checks
function _step_06()
{
    # restarts=5 - Nodes that upgrade
    log_step_upgrades 9 "running health checks"
    source "$NCTL"/sh/scenarios/common/health_checks.sh \
            errors='0' \
            equivocators='0' \
            doppels='0' \
            crashes=0 \
            restarts=5 \
            ejections=0
}

# Step 07: Terminate.
function _step_07()
{
    log_step_upgrades 7 "test successful - tidying up"

    source "$NCTL/sh/assets/teardown.sh"

    log_break
}

#######################################
# Returns auction info at a block
# identifier.
# Globals:
#   NCTL - path to nctl home directory.
######################################
function get_auction_state_at_block_1() {
    nctl-view-chain-auction-info block-height=1 | jq '.auction_state'
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
