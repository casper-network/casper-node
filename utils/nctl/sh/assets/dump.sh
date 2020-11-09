#!/usr/bin/env bash
#
# Tears down an entire network.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
# Arguments:
#   Network ordinal identifer.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)   
    case "$KEY" in
        net) net=${VALUE} ;;        
        *)   
    esac    
done

# Set defaults.
net=${net:-1}

#######################################
# Main
#######################################

log "network #$net: dumping transient assets ... please wait"

# Set paths.
path_assets=$NCTL/assets/net-$net
path_dump=$NCTL/dumps/net-$net

# Set dump directory.
if [ -d $path_dump ]; then
    rm -rf $path_dump
fi
mkdir -p $path_dump

# Set net vars.
source $path_assets/vars

# Dump chainspec.
cp $path_assets/chainspec/accounts.csv $path_dump/accounts.csv
cp $path_assets/chainspec/chainspec.toml $path_dump

# Dump daemon.
if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
    cp $path_assets/daemon/config/supervisord.conf $path_dump/daemon.conf
    cp $path_assets/daemon/logs/supervisord.log $path_dump/daemon.log
fi

# Dump faucet.
cp $path_assets/faucet/public_key_hex $path_dump/faucet-public_key_hex
cp $path_assets/faucet/public_key.pem $path_dump/faucet-public_key.pem
cp $path_assets/faucet/secret_key.pem $path_dump/faucet-secret_key.pem

# Dump nodes.
for node_id in $(seq 1 $NCTL_NET_NODE_COUNT)
do
    path_node=$path_assets/nodes/node-$node_id
    cp $path_node/config/node-config.toml $path_dump/node-$node_id-config.toml
    cp $path_node/keys/public_key_hex $path_dump/node-$node_id-public_key_hex
    cp $path_node/keys/public_key.pem $path_dump/node-$node_id-public_key.pem
    cp $path_node/keys/secret_key.pem $path_dump/node-$node_id-secret_key.pem
    cp $path_node/logs/stderr.log $path_dump/node-$node_id-stderr.log
    cp $path_node/logs/stdout.log $path_dump/node-$node_id-stdout.log
done

# Dump users.
for user_id in $(seq 1 $NCTL_NET_USER_COUNT)
do
    path_user=$path_assets/users/user-$user_id
    cp $path_user/public_key_hex $path_dump/user-$user_id-public_key_hex
    cp $path_user/public_key.pem $path_dump/user-$user_id-public_key.pem
    cp $path_user/secret_key.pem $path_dump/user-$user_id-secret_key.pem
done
