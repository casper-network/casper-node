#!/usr/bin/env bash
#
#######################################
# Compiles software.
# Globals:
#   NCTL - path to nctl home directory.
########################################

pull optional flag to run in debug mode
while getopts 'd' opt; do 
    case $opt in
        d ) export NCTL_COMPILE_TARGET="debug";;
        * ) echo "nctl-compile only accepts optional flag -d to compile in debug mode."
    esac
done

source "$NCTL"/sh/assets/compile_node.sh 
source "$NCTL"/sh/assets/compile_node_launcher.sh
source "$NCTL"/sh/assets/compile_client.sh
source "$NCTL"/sh/assets/compile_global_state_update_gen.sh