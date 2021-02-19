#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

unset LOG_LEVEL
unset NODE_ID
unset PROTOCOL_VERSION
unset ACTIVATE_ERA

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        version) PROTOCOL_VERSION=${VALUE} ;;
        era) ACTIVATE_ERA=${VALUE} ;;
        loglevel) LOG_LEVEL=${VALUE} ;;
        *) echo "Unknown argument '${KEY}'. Use 'version', 'era' or 'loglevel'." && exit 1;;
    esac
done

LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
export RUST_LOG=$LOG_LEVEL

function do_upgrade() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local NODE_COUNT=${3:-5}

    local PATH_TO_NET
    local PATH_TO_NODE

    PATH_TO_NET=$(get_path_to_net)

    # Set file.
    PATH_TO_CHAINSPEC_FILE="$PATH_TO_NET"/chainspec/chainspec.toml
    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"
    PATH_TO_UPGRADED_CHAINSPEC_FILE="$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/chainspec.toml
    cp "$PATH_TO_CHAINSPEC_FILE" "$PATH_TO_UPGRADED_CHAINSPEC_FILE"

    # Write contents.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CHAINSPEC_FILE');"
        "cfg['protocol']['version']='$PROTOCOL_VERSION'.replace('_', '.');"
        "cfg['protocol']['activation_point']['era_id']=$ACTIVATE_ERA;"
        "toml.dump(cfg, open('$PATH_TO_UPGRADED_CHAINSPEC_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    for NODE_ID in $(seq 1 $((NODE_COUNT * 2))); do
        PATH_TO_NODE=$(get_path_to_node "$NODE_ID")
        # Copy the casper-node binary
        mkdir -p "$PATH_TO_NODE"/bin/"$PROTOCOL_VERSION"
        cp "$NCTL_CASPER_HOME"/target/release/casper-node "$PATH_TO_NODE"/bin/"$PROTOCOL_VERSION"/
        # Copy chainspec
        mkdir -p "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/
        cp "$PATH_TO_UPGRADED_CHAINSPEC_FILE" "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/
        # Copy config file
        cp "$PATH_TO_NODE"/config/1_0_0/config.toml "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/
    done
}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Upgrade node(s).
log "upgrade node(s) begins ... please wait"
do_upgrade "$PROTOCOL_VERSION" "$ACTIVATE_ERA"
log "upgrade node(s) complete"
