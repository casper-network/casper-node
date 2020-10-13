#######################################
# Returns path to a network's supervisord.conf.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_net_supervisord_cfg() {
    echo $NCTL/assets/net-$1/daemon/config/supervisord.conf
}

#######################################
# Returns path to a network's socket file.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_net_supervisord_sock() {
    echo $NCTL/assets/net-$1/daemon/socket/supervisord.sock
}

#######################################
# Returns name of a node process managed via supervisord.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_process_name() {
    echo "casper-net-$1-node-$2"
}
