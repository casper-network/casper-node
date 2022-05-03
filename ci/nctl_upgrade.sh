#!/usr/bin/env bash
set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"

# Activate Environment
pushd "$ROOT_DIR"
source $(pwd)/utils/nctl/activate

# Call compile wrapper for client, launcher, and nctl-compile
bash -i "$ROOT_DIR/ci/nctl_compile.sh"

function main() {
    local TEST_ID=${1}
    local SKIP_SETUP=${2}
    if [ "$SKIP_SETUP" != "true" ]; then

        # NCTL Build
        pushd "$ROOT_DIR"
        nctl-compile

        # Clear Old Stages
        nctl-stage-teardown

        # Stage
        get_remotes
        stage_remotes
        build_from_settings_file
    fi

    if [ -z "$TEST_ID" ]; then
        # PR CI tests
        start_upgrade_scenario_1
        start_upgrade_scenario_3
    else
        start_upgrade_scenario_"$TEST_ID"
    fi
}

# Pulls down remotely staged file
# from s3 bucket to NCTL remotes directory.
function get_remotes() {
    local CI_JSON_CONFIG_FILE
    local PROTO_1

    CI_JSON_CONFIG_FILE="$NCTL/ci/ci.json"
    PROTO_1=$(jq -r '.nctl_upgrade_tests."protocol_1"' "$CI_JSON_CONFIG_FILE")
    nctl-stage-set-remotes "$PROTO_1"
}

# Sets up settings.sh for CI test.
# If local arg is passed it will skip this step
# and use whats currently in settings.sh
#   arg: local is for debug testing only
function stage_remotes() {
    local PATH_TO_STAGE

    PATH_TO_STAGE="$(get_path_to_stage 1)"
    dev_branch_settings "$PATH_TO_STAGE"
}

# Generates stage-1 directory for test execution
# Just here for a log message
function build_from_settings_file() {
    log "... setting build from settings.sh file"
    nctl-stage-build-from-settings
}

# Produces settings.sh needed for CI testing.
# It will always setup latest RC -> minor incremented by 1.
# i.e: if current RC is 1.2 then dev will be setup as 1.3
function dev_branch_settings() {
    local PATH_TO_STAGE=${1}
    local STARTING_VERSION=${2}
    local INCREMENT
    local RC_VERSION

    pushd "$(get_path_to_remotes)"
    RC_VERSION="$(ls --group-directories-first -d */ | sort -r | head -n 1 | tr -d '/')"

    [[ "$RC_VERSION" =~ (.*[^0-9])([0-9])(.)([0-9]+) ]] && INCREMENT="${BASH_REMATCH[1]}$((${BASH_REMATCH[2]} + 1))${BASH_REMATCH[3]}${BASH_REMATCH[4]}"

    RC_VERSION=$(echo "$RC_VERSION" | sed 's/\./\_/g')
    INCREMENT=$(echo "$INCREMENT" | sed 's/\./\_/g')

    # check if a version to start at was given
    if [ ! -z $STARTING_VERSION ]; then
        # overwrite start version
        RC_VERSION=$(echo "$STARTING_VERSION" | sed 's/\./\_/g')
    fi

    mkdir -p "$(get_path_to_stage '1')"

    cat <<EOF > "$(get_path_to_stage_settings 1)"
export NCTL_STAGE_SHORT_NAME="YOUR-SHORT-NAME"

export NCTL_STAGE_DESCRIPTION="YOUR-DESCRIPTION"

export NCTL_STAGE_TARGETS=(
    "${RC_VERSION}:remote"
    "${INCREMENT}:local"
)
EOF
    cat "$(get_path_to_stage_settings 1)"
    popd
}

# Kicks off the scenario
# Just here for a log message
function start_upgrade_scenario_1() {
    log "... Starting Upgrade Scenario 1"
    nctl-exec-upgrade-scenario-1
}

function start_upgrade_scenario_3() {
    log "... Starting Upgrade Scenario 3"
    nctl-exec-upgrade-scenario-3
}

function start_upgrade_scenario_4() {
    log "... Starting Upgrade Scenario 4"
    nctl-exec-upgrade-scenario-4
}

function start_upgrade_scenario_5() {
    log "... Starting Upgrade Scenario 5"
    nctl-exec-upgrade-scenario-5
}

function start_upgrade_scenario_6() {
    log "... Starting Upgrade Scenario 6"
    nctl-exec-upgrade-scenario-6
}

function start_upgrade_scenario_7() {
    log "... Starting Upgrade Scenario 7"
    nctl-exec-upgrade-scenario-7
}

function start_upgrade_scenario_8() {
    log "... Starting Upgrade Scenario 8"
    nctl-exec-upgrade-scenario-8
}

function start_upgrade_scenario_9() {
    log "... Starting Upgrade Scenario 9"
    nctl-exec-upgrade-scenario-9
}

function start_upgrade_scenario_10() {
    log "... Setting up custom starting version"
    local PATH_TO_STAGE

    PATH_TO_STAGE="$(get_path_to_stage 1)"

    log "... downloading remote for 1.3.0"
    nctl-stage-set-remotes "1.3.0"

    log "... tearing down old stages"
    nctl-stage-teardown

    log "... creating new stage"
    dev_branch_settings "$PATH_TO_STAGE" "1.3.0"
    build_from_settings_file

    log "... Starting Upgrade Scenario 10"
    nctl-exec-upgrade-scenario-10
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset TEST_ID
unset SKIP_SETUP

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        test_id) TEST_ID=${VALUE} ;;
        skip_setup) SKIP_SETUP=${VALUE} ;;
        *) ;;
    esac
done

main "$TEST_ID" "$SKIP_SETUP"
