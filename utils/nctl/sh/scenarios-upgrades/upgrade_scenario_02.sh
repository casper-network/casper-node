# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# Spins up a network, awaits for it to settle down and then performs a series of
# network upgrades.  At each step network behaviour is verified.

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
    local PATH_TO_STAGE
    local PROTOCOL_VERSION=""
    local UPGRADE_ID=0

    # Assert stage exists.
    PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")"
    if [ ! -d "$PATH_TO_STAGE" ]; then
        log "ERROR :: stage $STAGE_ID has not been built - cannot run scenario"
        exit 1
    fi

    # Iterate staged protocol versions:
    for FHANDLE in "$PATH_TO_STAGE/"*; do
        if [ -d "$FHANDLE" ]; then
            # ... spinup
            if [ "$PROTOCOL_VERSION" == "" ]; then
                _spinup "$STAGE_ID" "$PATH_TO_STAGE" "$(basename "$FHANDLE")"
            # ... upgrade
            else
                UPGRADE_ID=$((UPGRADE_ID + 1))
                _upgrade "$UPGRADE_ID" "$STAGE_ID" "$(basename "$FHANDLE")" "$PROTOCOL_VERSION"
            fi
            PROTOCOL_VERSION="$(basename "$FHANDLE")"
        fi
    done

    # Tidy up.
    log_break
    log "test successful - tidying up"
    source "$NCTL/sh/assets/teardown.sh"
    log_break
}

# Spinup: start network from pre-built stage.
function _spinup()
{
    local STAGE_ID=${1}
    local PATH_TO_STAGE=${2}
    local PROTOCOL_VERSION=${3}

    _spinup_step_01 "$STAGE_ID"
    _spinup_step_02
    _spinup_step_03
    _spinup_step_04
}

# Spinup: step 01: Start network from pre-built stage.
function _spinup_step_01()
{
    local STAGE_ID=${1}

    log_step 1 "starting network from stage $STAGE_ID" "SPINUP"

    source "$NCTL/sh/assets/setup_from_stage.sh" stage="$STAGE_ID"
    source "$NCTL/sh/node/start.sh" node="all"
}

# Spinup: step 02: Await era-id >= 1.
function _spinup_step_02()
{
    log_step 2 "awaiting genesis era completion" "SPINUP"

    await_until_era_n 1
    log " ... chain @ era-$(get_chain_era) :: height-$(get_chain_height)"
}

# Spinup: step 03: Populate global state -> native + wasm transfers.
function _spinup_step_03()
{
    log_step 3 "dispatching deploys to populate global state" "SPINUP"

    log "... 100 native transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_native.sh" \
        transfers=100 interval=0.0 verbose=false

    log "... 100 wasm transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_wasm.sh" \
        transfers=100 interval=0.0 verbose=false
}

# Spinup: step 04: Await era-id += 1.
function _spinup_step_04()
{
    log_step 4 "awaiting next era" "SPINUP"

    await_n_eras 1
}

# Upgrade: Progress network to next upgrade from pre-built stage.
function _upgrade()
{
    local UPGRADE_ID=${1}
    local STAGE_ID=${2}
    local PROTOCOL_VERSION=${3}
    local PROTOCOL_VERSION_PREVIOUS=${4}

    _upgrade_step_01 "$UPGRADE_ID" "$STAGE_ID" "$PROTOCOL_VERSION" "$PROTOCOL_VERSION_PREVIOUS"
    _upgrade_step_02 "$UPGRADE_ID"
    _upgrade_step_03 "$UPGRADE_ID"
    _upgrade_step_04 "$UPGRADE_ID"
    _upgrade_step_05 "$UPGRADE_ID"
}

# Upgrade: Upgrade network from stage.
function _upgrade_step_01()
{
    local UPGRADE_ID=${1}
    local STAGE_ID=${2}
    local PROTOCOL_VERSION=${3}
    local PROTOCOL_VERSION_PREVIOUS=${4}

    log_step 1 "upgrading network from stage ($STAGE_ID) @ $PROTOCOL_VERSION_PREVIOUS -> $PROTOCOL_VERSION" "UPGRADE $UPGRADE_ID:"

    log "... setting upgrade assets"
    source "$NCTL/sh/assets/upgrade_from_stage.sh" stage="$STAGE_ID" verbose=false

    log "... awaiting upgrade"
    await_n_eras 4
}

# Upgrade: Populate global state -> native + wasm transfers.
function _upgrade_step_02()
{
    local UPGRADE_ID=${1}

    log_step 2 "dispatching deploys to populate global state" "UPGRADE $UPGRADE_ID:"

    log "... ... 100 native transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_native.sh" \
        transfers=100 interval=0.0 verbose=false

    log "... ... 100 wasm transfers"
    source "$NCTL/sh/contracts-transfers/do_dispatch_wasm.sh" \
        transfers=100 interval=0.0 verbose=false
}

# Upgrade: Await era-id += 1.
function _upgrade_step_03()
{
    log_step 4 "awaiting next era" "UPGRADE $UPGRADE_ID:"

    await_n_eras 1
}

# Upgrade: Assert chain is live.
function _upgrade_step_04()
{
    local UPGRADE_ID=${1}

    log_step 3 "asserting chain liveness" "UPGRADE $UPGRADE_ID:"

    if [ "$(get_count_of_up_nodes)" != "$(get_count_of_genesis_nodes)" ]; then
        log "ERROR :: protocol upgrade failure - >= 1 nodes have stopped"
        exit 1
    fi
}

# Upgrade: Assert chain is progressing at all nodes.
function _upgrade_step_05()
{
    local UPGRADE_ID=${1}
    local HEIGHT_1
    local HEIGHT_2
    local NODE_ID

    log_step 4 "asserting chain progression" "UPGRADE $UPGRADE_ID:"

    HEIGHT_1=$(get_chain_height)
    await_n_blocks 2

    for NODE_ID in $(seq 1 "$(get_count_of_genesis_nodes)")
    do
        HEIGHT_2=$(get_chain_height "$NODE_ID")
        if [ "$HEIGHT_2" != "N/A" ] && [ "$HEIGHT_2" -le "$HEIGHT_1" ]; then
            log "ERROR :: protocol upgrade failure - >= 1 nodes have stalled"
            exit 1
        fi
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset _STAGE_ID

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
