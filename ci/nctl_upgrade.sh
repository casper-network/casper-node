#!/usr/bin/env bash
set -e

function main() {
#    if [ -z "${DRONE}" ]; then
#        echo "Must be run on Drone!"
#        exit 1
#    fi

    ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"

    VERSION_ARRAY=(
                $(aws s3 ls s3://nctl.casperlabs.io/ | \
                    awk '{ print $2 }' | \
                    grep 'v\|rel' | \
                    tr -d [:alpha:] | \
                    tr -d '-' | \
                    tr -d '/'
                )
    )

    pushd $ROOT_DIR
    source ./utils/nctl/activate

    nctl-stage-set-remotes "${VERSION_ARRAY[*]}"

    PATH_TO_STAGE="$(get_path_to_stage 1)"

    if [ "$1" != "local" ]; then
        dev_branch_settings "$PATH_TO_STAGE"
    fi
    nctl-stage-build-from-settings
    nctl-exec-upgrade-scenario-1
    
}

function dev_branch_settings() {
    local PATH_TO_STAGE=${1}
    local INCREMENT
    local RC_VERSION
    local STAGING_FILE

    pushd $(get_path_to_remotes)
    RC_VERSION="$(ls --group-directories-first -d */ | sort -r | head -n 1 | tr -d '/')"

    [[ "$RC_VERSION" =~ (.*[^0-9])([0-9])(.)([0-9]+) ]] && INCREMENT="${BASH_REMATCH[1]}$((${BASH_REMATCH[2]} + 1))${BASH_REMATCH[3]}${BASH_REMATCH[4]}"

    RC_VERSION="$(echo $RC_VERSION | sed 's/\./\_/g')"
    INCREMENT="$(echo $INCREMENT | sed 's/\./\_/g')"

    mkdir -p $(get_path_to_stage '1')

    cat <<EOF > $(get_path_to_stage_settings '1') 
export NCTL_STAGE_SHORT_NAME="YOUR-SHORT-NAME"

export NCTL_STAGE_DESCRIPTION="YOUR-DESCRIPTION"

export NCTL_STAGE_TARGETS=(
    "${RC_VERSION}:remote"
    "${INCREMENT}:local"
)
EOF
    cat $(get_path_to_stage_settings 1)
    popd
}

main $1
