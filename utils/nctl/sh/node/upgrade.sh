#!/usr/bin/env bash

source "$NCTL"/sh/assets/upgrade.sh

unset NODE_ID
unset PROTOCOL_VERSION
unset ACTIVATE_ERA

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        version) PROTOCOL_VERSION=${VALUE} ;;
        era) ACTIVATE_ERA=${VALUE} ;;
        *) echo "Unknown argument '${KEY}'. Use 'version', 'era' or 'loglevel'." && exit 1 ;;
    esac
done

function do_upgrade() {
    local PROTOCOL_VERSION=${1:-"2_0_0"}
    local ACTIVATE_ERA=${2}

    for NODE_ID in $(seq 1 "$(get_count_of_nodes)"); do
        _upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID"
    done
}

#######################################
# Upgrades all nodes in the network to a specified protocol version.
# Does not modify the chainspec file in any way that has an influence on the network,
# except for setting required entries for the upgrade to take place.
#
# `version` arg should use underscores, e.g. `2_0_0`
#######################################

#
# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Upgrade node(s).
log "upgrade node(s) begins ... please wait"
shopt -s expand_aliases
do_upgrade "$PROTOCOL_VERSION" "$ACTIVATE_ERA"
log "upgrade node(s) complete"
