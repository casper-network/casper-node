#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

log "transient asset dump ... starts"

# Set paths.
PATH_TO_NET=$(get_path_to_net)
PATH_TO_DUMP=$(get_path_to_net_dump)

# Set dump directory.
rm -rf "$PATH_TO_DUMP"
mkdir -p "$PATH_TO_DUMP"

# Dump daemon.
if [ "$NCTL_DAEMON_TYPE" = "supervisord" ]; then
    mkdir -p "$PATH_TO_DUMP"/daemon
    cp -r "$PATH_TO_NET"/daemon/config "$PATH_TO_DUMP"/daemon
    cp -r "$PATH_TO_NET"/daemon/logs "$PATH_TO_DUMP"/daemon
fi

# Dump faucet.
cp -r "$PATH_TO_NET"/faucet "$PATH_TO_DUMP"

# Dump nodes.
for NODE_ID in $(seq 1 "$(get_count_of_started_nodes)")
do
    PATH_TO_NODE_DUMP="$PATH_TO_DUMP/node-$NODE_ID"
    cp -r $(get_path_to_node_config $NODE_ID) "$PATH_TO_NODE_DUMP"
    cp -r $(get_path_to_node_keys $NODE_ID) "$PATH_TO_NODE_DUMP"
    cp -r $(get_path_to_node_logs $NODE_ID) "$PATH_TO_NODE_DUMP"
done

# Remove socket files which could exist in nodes' config folders.
find "$PATH_TO_DUMP" -type s -delete

# Dump users.
cp -r "$PATH_TO_NET"/users/* "$PATH_TO_DUMP"

log "transient asset dump ... complete"
