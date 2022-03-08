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

# Pulls down all remotely staged files
# from s3 bucket to NCTL remotes directiory.
function get_remotes() {
    local VERSION_ARRAY

    log "... downloading remote files and binaries"

    if [ -z "$AWS_SECRET_ACCESS_KEY" ] || [ -z "$AWS_ACCESS_KEY_ID" ]; then
        log "ERROR: AWS KEYS needed to run. Contact SRE."
        exit 1
    fi

    VERSION_ARRAY=(
                $(aws s3 ls s3://nctl.casperlabs.io/ | \
                    awk '{ print $2 }' | \
                    grep 'v\|rel' | \
                    tr -d "[:alpha:]" | \
                    tr -d '-' | \
                    tr -d '/'
                )
    )

    if [ -z "${VERSION_ARRAY[*]}" ]; then
        log "ERROR: Version Array was blank. Exiting."
        exit 1
    fi

    nctl-stage-set-remotes "${VERSION_ARRAY[*]}"
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
    local INCREMENT
    local RC_VERSION

    pushd "$(get_path_to_remotes)"
    RC_VERSION="$(ls --group-directories-first -d */ | sort -r | head -n 1 | tr -d '/')"

    [[ "$RC_VERSION" =~ (.*[^0-9])([0-9])(.)([0-9]+) ]] && INCREMENT="${BASH_REMATCH[1]}$((${BASH_REMATCH[2]} + 1))${BASH_REMATCH[3]}${BASH_REMATCH[4]}"

    RC_VERSION=$(echo "$RC_VERSION" | sed 's/\./\_/g')
    INCREMENT=$(echo "$INCREMENT" | sed 's/\./\_/g')

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
