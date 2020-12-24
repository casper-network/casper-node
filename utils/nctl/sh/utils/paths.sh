#######################################
# Returns path to a binary file.
# Arguments:
#   Binary file name.
#######################################
function get_path_to_binary()
{
    local FILENAME=${1}    

    echo $(get_path_to_net)/bin/$FILENAME
}

#######################################
# Returns path to client binary.
#######################################
function get_path_to_client()
{
    echo $(get_path_to_binary "casper-client")
}

#######################################
# Returns path to a smart contract.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Contract wasm file name.
#######################################
function get_path_to_contract()
{
    local FILENAME=${1}

    echo $(get_path_to_binary $FILENAME)
}

#######################################
# Returns path to a network faucet.
#######################################
function get_path_to_faucet()
{
    echo $(get_path_to_net)/faucet
}

#######################################
# Returns path to a network's assets.
# Globals:
#   NCTL - path to nctl home directory.
#######################################
function get_path_to_net()
{
    local NET_ID=${NET_ID:-1}

    echo $NCTL/assets/net-$NET_ID
}

#######################################
# Returns path to a network's dump folder.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_net_dump()
{
    local NET_ID=${NET_ID:-1}

    echo $NCTL/dumps/net-$NET_ID
}

#######################################
# Returns path to a network's supervisord config file.
#######################################
function get_path_net_supervisord_cfg()
{
    echo $(get_path_to_net)/daemon/config/supervisord.conf
}

#######################################
# Returns path to a network's supervisord socket file.
#######################################
function get_path_net_supervisord_sock()
{
    echo $(get_path_to_net)/daemon/socket/supervisord.sock
}

#######################################
# Returns path to a node's assets.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_path_to_node()
{
    local NODE_ID=${1} 

    echo $(get_path_to_net)/nodes/node-$NODE_ID
}

#######################################
# Returns path to a secret key.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function get_path_to_secret_key()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2}

    if [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        echo $(get_path_to_faucet)/secret_key.pem
    elif [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_NODE ]; then
        echo $(get_path_to_node $ACCOUNT_IDX)/keys/secret_key.pem
    elif [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_USER ]; then
        echo $(get_path_to_user $ACCOUNT_IDX)/secret_key.pem
    fi
}

#######################################
# Returns path to a user's assets.
# Arguments:
#   User ordinal identifier.
#######################################
function get_path_to_user()
{
    local USER_ID=${1}

    echo $(get_path_to_net)/users/user-$USER_ID
}
