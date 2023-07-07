#!/usr/bin/env bash
# ----------------------------------------------------------------
# Synopsis.
# ----------------------------------------------------------------

# 1. Start v1 running at current mainnet commit.
# 2. Wait for genesis era to complete.
# 3. Delegate from an unused account.
# 4. Wait for the auction delay to take effect.
# 5. Asserts delegation is in auction info.
# 6. Undelegate.
# 7. Perform an emergency upgrade and assert validator set has changed.
# 8. Asserts delegatee is NO LONGER in auction info.
# 9. Run health checks.
# 10. Successful test cleanup.

# ----------------------------------------------------------------
# Imports.
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"
source "$NCTL/sh/node/svc_$NCTL_DAEMON_TYPE.sh"
source "$NCTL/sh/assets/upgrade.sh"
source "$NCTL/sh/scenarios/common/itst.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Main entry point.
function _main()
{
    local STAGE_ID=${1}

    if [ ! -d "$(get_path_to_stage "$STAGE_ID")" ]; then
        log "ERROR :: stage $STAGE_ID has not been built - cannot run scenario"
        exit 1
    fi

    _step_01 "$STAGE_ID"
    _step_02

    # Set initial protocol version for use later.
    INITIAL_PROTOCOL_VERSION=$(get_node_protocol_version 1)
    _step_03
    _step_04
    _step_05
    _step_06
    _step_07
    _step_08
    _step_09
    _step_10
}

function _custom_validators_state_update_config()
{
    local PUBKEY
    TEMPFILE=$(mktemp)

    echo 'only_listed_validators = false' >> $TEMPFILE

    cat << EOF >> $TEMPFILE
[[accounts]]
public_key = "$(get_node_public_key_hex_extended 6)"

[accounts.validator]
bonded_amount = "$((6 + 1000000000000000))"
delegation_rate = 100

[[accounts]]
public_key = "$(get_node_public_key_hex_extended 1)"

[accounts.validator]
bonded_amount = "0"
delegation_rate = 100
EOF

    echo $TEMPFILE
}

# Step 01: Start v1 running at current mainnet commit.
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
            chainspec_path="$NCTL/overrides/upgrade_scenario_11.pre.chainspec.toml.in" \
            accounts_path="$NCTL/overrides/upgrade_scenario_11.pre.accounts.toml"
    source "$NCTL/sh/node/start.sh" node=all
}

# Step 02: Wait for genesis era to complete.
function _step_02()
{
    log_step_upgrades 2 "awaiting genesis era completion"

    do_await_genesis_era_to_complete 'false'
}

# Step 03: Delegate from an unused account.
function _step_03()
{
    local NODE_ID=${1:-'5'}
    local ACCOUNT_ID=${2:-'7'}
    local AMOUNT=${3:-'500000000000'}
    local NODE_PUBLIC_KEY

    log_step_upgrades 3 "Delegating $AMOUNT from account-$ACCOUNT_ID to validator-$NODE_ID"

    source "$NCTL/sh/contracts-auction/do_delegate.sh" \
            amount="$AMOUNT" \
            delegator="$ACCOUNT_ID" \
            validator="$NODE_ID"

    NODE_PUBLIC_KEY=$(nctl-view-node-status node=$NODE_ID | grep our_public_signing_key | cut -c 30-95)
    log "Delegated to validator: $NODE_PUBLIC_KEY"
}

# Step 04: Wait for the auction delay to take effect.
function _step_04()
{
    log_step_upgrades 4 "Awaiting Auction_Delay = 1 + 1"
    nctl-await-n-eras offset='2' sleep_interval='2.0' timeout='300'
}

# Step 05: Asserts delegation is in auction info.
function _step_05()
{
    local USER_ID=${1:-'7'}
    local USER_PATH
    local HEX
    local AUCTION_INFO_FOR_HEX
    local TIMEOUT_SEC

    TIMEOUT_SEC='0'

    USER_PATH=$(get_path_to_user "$USER_ID")
    HEX=$(cat "$USER_PATH"/public_key_hex | tr '[:upper:]' '[:lower:]')

    log_step_upgrades 5 "Asserting user-$USER_ID is a delegatee"

    while [ "$TIMEOUT_SEC" -le "60" ]; do
        AUCTION_INFO_FOR_HEX=$(nctl-view-chain-auction-info | jq --arg node_hex "$HEX" '.auction_state.bids[]| select(.bid.delegators[].public_key | ascii_downcase == $node_hex)')
        if [ ! -z "$AUCTION_INFO_FOR_HEX" ]; then
            log "... user-$USER_ID found in auction info delegators!"
            log "... public_key_hex: $HEX"
            break
        else
            TIMEOUT_SEC=$((TIMEOUT_SEC + 1))
            log "... timeout=$TIMEOUT_SEC: delegatee not yet detected"
            sleep 1
            if [ "$TIMEOUT_SEC" = '60' ]; then
                log "ERROR: Could not find $HEX in auction info delegators!"
                exit 1
            fi
        fi
    done

    nctl-await-n-eras offset='1' sleep_interval='2.0' timeout='300'
}

# Step 06: Undelegate.
function _step_06()
{
    local NODE_ID=${1:-'5'}
    local ACCOUNT_ID=${2:-'7'}
    local AMOUNT=${3:-'500000000000'}

    log_step_upgrades 6 "Undelegating $AMOUNT to account-$ACCOUNT_ID from validator-$NODE_ID"

    source "$NCTL/sh/contracts-auction/do_delegate_withdraw.sh" \
            amount="$AMOUNT" \
            delegator="$ACCOUNT_ID" \
            validator="$NODE_ID"

    nctl-await-n-eras offset='2' sleep_interval='2.0' timeout='300'
}

# Step 07: Perform an emergency upgrade and assert validator set has changed.
function _step_07()
{
    local ACTIVATE_ERA
    local ERA_ID
    local SWITCH_BLOCK
    local STATE_HASH
    local TRUSTED_HASH
    local PROTOCOL_VERSION
    local GS_PARAMS
    local STATE_SOURCE_PATH
    local AUCTION_INFO
    local OLD_VALIDATORS_ERA_ID
    local NEW_VALIDATORS_ERA_ID
    local OLD_VALIDATORS_PUBLIC_KEYS
    local NEW_VALIDATORS_PUBLIC_KEYS

    ACTIVATE_ERA="$(get_chain_era)"
    ERA_ID=$((ACTIVATE_ERA - 1))
    SWITCH_BLOCK=$(get_switch_block "1" "32" "" "$ERA_ID")
    STATE_HASH=$(echo "$SWITCH_BLOCK" | jq -r '.header.state_root_hash')
    TRUSTED_HASH=$(echo "$SWITCH_BLOCK" | jq -r '.hash')
    PROTOCOL_VERSION='2_0_0'
    STATE_SOURCE_PATH=$(get_path_to_node 1)
    GS_PARAMS="generic -d ${STATE_SOURCE_PATH}/storage/$(get_chain_name) -s ${STATE_HASH} $(_custom_validators_state_update_config)"

    log_step_upgrades 7 "Emergency restart with validator swap"
    log "...emergency upgrade activation era = $ACTIVATE_ERA"
    log "...state hash = $STATE_HASH"
    log "...trusted hash = $TRUSTED_HASH"
    log "...new protocol version = $PROTOCOL_VERSION"

    AUCTION_INFO=$(nctl-view-chain-auction-info)
    OLD_VALIDATORS_ERA_ID=$(echo $AUCTION_INFO | jq '.auction_state.era_validators' | jq '.[1]' | jq '.era_id')
    OLD_VALIDATORS_PUBLIC_KEYS=$(echo $AUCTION_INFO | jq '.auction_state.era_validators' | jq '.[1]' | jq '.validator_weights[].public_key' | sort)

    do_node_stop_all

    _generate_global_state_update "$PROTOCOL_VERSION" "$STATE_HASH" 1 "$(get_count_of_genesis_nodes)" "$GS_PARAMS"

    for NODE_ID in $(seq 1 "$(get_count_of_nodes)"); do
        log "...preparing $NODE_ID"
        _emergency_upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID" "$STATE_HASH" 1 "$(get_count_of_genesis_nodes)" "$NCTL_CASPER_HOME/resources/local/config.toml" "$NCTL/overrides/upgrade_scenario_10.post.chainspec.toml.in" "false"
        log "...starting $NODE_ID"
        do_node_start "$NODE_ID" "$TRUSTED_HASH"
    done

    nctl-await-n-eras offset='3' sleep_interval='2.0' timeout='300'

    AUCTION_INFO=$(nctl-view-chain-auction-info)
    NEW_VALIDATORS_ERA_ID=$(echo $AUCTION_INFO | jq '.auction_state.era_validators' | jq '.[1]' | jq '.era_id')
    NEW_VALIDATORS_PUBLIC_KEYS=$(echo $AUCTION_INFO | jq '.auction_state.era_validators' | jq '.[1]' | jq '.validator_weights[].public_key' | sort)

    if [[ "$OLD_VALIDATORS_PUBLIC_KEYS" == "$NEW_VALIDATORS_PUBLIC_KEYS" ]]
    then
        log "ERROR :: Validator set did not change after an emergency upgrade"
        log "ERROR :: Old validators (era=$OLD_VALIDATORS_ERA_ID): $OLD_VALIDATORS_PUBLIC_KEYS"
        log "ERROR :: New validators (era=$NEW_VALIDATORS_ERA_ID): $NEW_VALIDATORS_PUBLIC_KEYS"
        exit 1
    else
        log "Validator set has been correctly updated"
    fi

    rm -f $TEMPFILE
}

# Step 08: Asserts delegatee is NO LONGER in auction info.
function _step_08()
{
    local USER_ID=${1:-'7'}
    local USER_PATH
    local HEX
    local AUCTION_INFO_FOR_HEX

    USER_PATH=$(get_path_to_user "$USER_ID")
    HEX=$(cat "$USER_PATH"/public_key_hex | tr '[:upper:]' '[:lower:]')
    AUCTION_INFO_FOR_HEX=$(nctl-view-chain-auction-info | jq --arg node_hex "$HEX" '.auction_state.bids[]| select(.bid.delegators[].delegator_public_key | ascii_downcase == $node_hex)')

    log_step_upgrades 8 "Asserting user-$USER_ID is NOT a delegatee"

    if [ ! -z "$AUCTION_INFO_FOR_HEX" ]; then
        log "ERROR: user-$USER_ID found in auction info delegators!"
        log "... public_key_hex: $HEX"
        exit 1
    else
        log "... Could not find $HEX in auction info delegators! [expected]"
    fi
}

# Step 9: Run health checks.
function _step_09()
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

# Step 10: Successful test cleanup.
function _step_10()
{
    log_step_upgrades 10 "test successful - tidying up"

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
