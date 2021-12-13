#!/bin/bash

rm -rf $NCTL/remotes $NCTL/stages

nctl-compile

nctl-stage-set-remotes 1.4.1

mkdir -p "$(get_path_to_stage 1)"

cat <<EOF > "$(get_path_to_stage_settings 1)"
export NCTL_STAGE_SHORT_NAME="YOUR-SHORT-NAME"
export NCTL_STAGE_DESCRIPTION="YOUR-DESCRIPTION"
export NCTL_STAGE_TARGETS=(
    "1_4_1:remote"
    "1_5_0:local"
)
EOF

nctl-stage-build-from-settings

bash -i $NCTL/sh/scenarios-upgrades/upgrade_scenario_04.sh