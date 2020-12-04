#!/usr/bin/env bash
#
# Tears down an entire network.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
# Arguments:
#   Network ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset NET_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)   
    case "$KEY" in
        net) NET_ID=${VALUE} ;;        
        *)   
    esac    
done

# Set defaults.
NET_ID=${NET_ID:-1}

#######################################
# Imports
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import net vars.
source $(get_path_to_net_vars $NET_ID)

#######################################
# Main
#######################################

log "net-$NET_ID: dumping transient assets ... please wait"

# Set paths.
PATH_TO_NET=$(get_path_to_net $NET_ID)
PATH_TO_DUMP=$(get_path_to_net_dump $NET_ID)

# Set dump directory.
if [ -d $PATH_TO_DUMP ]; then
    rm -rf $PATH_TO_DUMP
fi
mkdir -p $PATH_TO_DUMP

# Dump chainspec.
cp $PATH_TO_NET/chainspec/accounts.csv $PATH_TO_DUMP/accounts.csv
cp $PATH_TO_NET/chainspec/chainspec.toml $PATH_TO_DUMP

# Dump daemon.
if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
    cp $PATH_TO_NET/daemon/config/supervisord.conf $PATH_TO_DUMP/daemon.conf
    cp $PATH_TO_NET/daemon/logs/supervisord.log $PATH_TO_DUMP/daemon.log
fi

# Dump faucet.
cp $PATH_TO_NET/faucet/public_key_hex $PATH_TO_DUMP/faucet-public_key_hex
cp $PATH_TO_NET/faucet/public_key.pem $PATH_TO_DUMP/faucet-public_key.pem
cp $PATH_TO_NET/faucet/secret_key.pem $PATH_TO_DUMP/faucet-secret_key.pem

# Dump nodes.
for IDX in $(seq 1 $NCTL_NET_NODE_COUNT)
do
    PATH_TO_NODE=$(get_path_to_node $NET_ID $IDX)
    cp $PATH_TO_NODE/config/node-config.toml $PATH_TO_DUMP/node-$IDX-config.toml
    cp $PATH_TO_NODE/keys/public_key_hex $PATH_TO_DUMP/node-$IDX-public_key_hex
    cp $PATH_TO_NODE/keys/public_key.pem $PATH_TO_DUMP/node-$IDX-public_key.pem
    cp $PATH_TO_NODE/keys/secret_key.pem $PATH_TO_DUMP/node-$IDX-secret_key.pem
    cp $PATH_TO_NODE/logs/stderr.log $PATH_TO_DUMP/node-$IDX-stderr.log
    cp $PATH_TO_NODE/logs/stdout.log $PATH_TO_DUMP/node-$IDX-stdout.log
done

# Dump users.
for IDX in $(seq 1 $NCTL_NET_USER_COUNT)
do
    PATH_TO_USER=$(get_path_to_user $NET_ID $IDX)
    cp $PATH_TO_USER/public_key_hex $PATH_TO_DUMP/user-$IDX-public_key_hex
    cp $PATH_TO_USER/public_key.pem $PATH_TO_DUMP/user-$IDX-public_key.pem
    cp $PATH_TO_USER/secret_key.pem $PATH_TO_DUMP/user-$IDX-secret_key.pem
done
