#!/usr/bin/env bash

# ###############################################################
# UTILS: constants
# ###############################################################

# A type of actor representing a participating node.
export NCTL_ACCOUNT_TYPE_FAUCET="faucet"

# A type of actor representing a participating node.
export NCTL_ACCOUNT_TYPE_NODE="node"

# A type of actor representing a user.
export NCTL_ACCOUNT_TYPE_USER="user"

# Base RPC server port number.
export NCTL_BASE_PORT_RPC=40000

# Base JSON server port number.
export NCTL_BASE_PORT_REST=50000

# Base event server port number.
export NCTL_BASE_PORT_SSE=60000

# Base network server port number.
export NCTL_BASE_PORT_NETWORK=34452

# Set of chain system contracts.
export NCTL_CONTRACTS_SYSTEM=(
    auction_install.wasm
    mint_install.wasm
    pos_install.wasm
    standard_payment_install.wasm
)

# Set of chain system contracts.
export NCTL_CONTRACTS_CLIENT=(
    add_bid.wasm
    delegate.wasm
    transfer_to_account_u512.wasm
    transfer_to_account_u512_stored.wasm
    undelegate.wasm
    withdraw_bid.wasm
)

# Default amount used when making auction bids.
export NCTL_DEFAULT_AUCTION_BID_AMOUNT=1000000000   # (1e9)

# Default amount used when delegating.
export NCTL_DEFAULT_AUCTION_DELEGATE_AMOUNT=1000000000   # (1e9)

# Default motes to pay for consumed gas.
export NCTL_DEFAULT_GAS_PAYMENT=1000000000   # (1e9)

# Default gas price multiplier.
export NCTL_DEFAULT_GAS_PRICE=10

# Default amount used when making transfers.
export NCTL_DEFAULT_TRANSFER_AMOUNT=1000000000   # (1e9)

# Intitial balance of faucet account.
export NCTL_INITIAL_BALANCE_FAUCET=1000000000000000000000000000000000   # (1e33)

# Intitial balance of user account.
export NCTL_INITIAL_BALANCE_USER=1000000000000000000   # (1e18)

# Intitial balance of validator account.
export NCTL_INITIAL_BALANCE_VALIDATOR=1000000000000000000000000000000000   # (1e33)

# Base weight applied to a validator at genesis.
export NCTL_VALIDATOR_BASE_WEIGHT=1000000000000000   # (1e15)

# Name of process group: boostrap validators.
export NCTL_PROCESS_GROUP_1=validators-1

# Name of process group: genesis validators.
export NCTL_PROCESS_GROUP_2=validators-2

# Name of process group: non-genesis validators.
export NCTL_PROCESS_GROUP_3=validators-3

# ###############################################################
# UTILS: helper functions
# ###############################################################

# OS types.
declare _OS_LINUX="linux"
declare _OS_LINUX_REDHAT="$_OS_LINUX-redhat"
declare _OS_LINUX_SUSE="$_OS_LINUX-suse"
declare _OS_LINUX_ARCH="$_OS_LINUX-arch"
declare _OS_LINUX_DEBIAN="$_OS_LINUX-debian"
declare _OS_MACOSX="macosx"
declare _OS_UNKNOWN="unknown"

#######################################
# Returns OS type.
# Globals:
#   OSTYPE: type of OS being run.
#######################################
function get_os()
{
	if [[ "$OSTYPE" == "linux-gnu" ]]; then
		if [ -f /etc/redhat-release ]; then
			echo $_OS_LINUX_REDHAT
		elif [ -f /etc/SuSE-release ]; then
			echo $_OS_LINUX_SUSE
		elif [ -f /etc/arch-release ]; then
			echo $_OS_LINUX_ARCH
		elif [ -f /etc/debian_version ]; then
			echo $_OS_LINUX_DEBIAN
		fi
	elif [[ "$OSTYPE" == "darwin"* ]]; then
		echo $_OS_MACOSX
	else
		echo $_OS_UNKNOWN
	fi
}

#######################################
# Wraps standard echo by adding application prefix.
#######################################
function log ()
{
    local MSG=${1}
    local NOW=`date +%Y-%m-%dT%H:%M:%S.%6N`
    local TABS=''

    if [ "$1" ]; then
        if [ "$2" ]; then
            for ((i=0; i<$2; i++))
            do
                TABS+='\t'
            done
            echo $NOW" [INFO] [$$] NCTL :: "$TABS$MSG
        else
            echo $NOW" [INFO] [$$] NCTL :: "$MSG
        fi
    else
        echo $NOW" [INFO] [$$] NCTL :: "
    fi
}

#######################################
# Wraps standard echo by adding application error prefix.
#######################################
function log_error ()
{
    local ERR=${1}
    local NOW=`date +%Y-%m-%dT%H:%M:%S.%6N`
    local TABS=''

    # Emit log message.
    if [ "$1" ]; then
        if [ "$2" ]; then
            for ((i=0; i<$2; i++))
            do
                TABS+='\t'
            done
            echo $NOW" [ERROR] [$$] NCTL :: "$TABS$ERR
        else
            echo $NOW" [ERROR] [$$] NCTL :: "$ERR
        fi
    else
        echo $NOW" [ERROR] [$$] NCTL :: "
    fi
}

#######################################
# Wraps pushd command to suppress stdout.
#######################################
function pushd ()
{
    command pushd "$@" > /dev/null
}

#######################################
# Wraps popd command to suppress stdout.
#######################################
function popd ()
{
    command popd "$@" > /dev/null
}

#######################################
# Forces a directory delete / recreate.
# Arguments:
#   Directory to be reset / recreated.
#######################################
function resetd () {
    local DPATH=${1}

    if [ -d $DPATH ]; then
        rm -rf $DPATH
    fi
    mkdir -p $DPATH
}

# ###############################################################
# UTILS: getter functions
# ###############################################################

#######################################
# Returns an on-chain account hash.
# Arguments:
#   Data to be hashed.
#######################################
function get_account_hash() {
    local ACCOUNT_KEY=${1}
    local ACCOUNT_PBK=${ACCOUNT_KEY:2}

    local SCRIPT=(
        "import hashlib;"
        "as_bytes=bytes('ed25519', 'utf-8') + bytearray(1) + bytes.fromhex('$ACCOUNT_PBK');"
        "h=hashlib.blake2b(digest_size=32);"
        "h.update(as_bytes);"
        "print(h.digest().hex());"
     )

    python3 -c "${SCRIPT[*]}"
}

#######################################
# Returns an account key.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function get_account_key() {
    local NET_ID=${1}
    local ACCOUNT_TYPE=${2}
    local ACCOUNT_IDX=${3}

    if [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        echo `cat $(get_path_to_faucet $NET_ID)/public_key_hex`
    elif [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_NODE ]; then
        echo `cat $(get_path_to_node $NET_ID $ACCOUNT_IDX)/keys/public_key_hex`
    elif [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_USER ]; then
        echo `cat $(get_path_to_user $NET_ID $ACCOUNT_IDX)/public_key_hex`
    fi
}

#######################################
# Returns an account prefix used when logging.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
# Arguments:
#   Network ordinal identifier.
#   Account type.
#   Account index (optional).
#######################################
function get_account_prefix() {
    local NET_ID=${1}
    local ACCOUNT_TYPE=${2}
    local ACCOUNT_IDX=${3:-}

    local prefix="net-$NET_ID.$ACCOUNT_TYPE"
    if [ $ACCOUNT_TYPE != $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        prefix=$prefix"-"$ACCOUNT_IDX
    fi 

    echo $prefix
}

#######################################
# Returns a network known addresses - i.e. those of bootstrap nodes.
# Globals:
#   NCTL_BASE_PORT_NETWORK - base network port number.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_bootstrap_known_address() {
    local NET_ID=${1}    
    local NODE_ID=${2}
    local NODE_PORT=$(($NCTL_BASE_PORT_NETWORK + ($NET_ID * 100) + $NODE_ID))

    echo "127.0.0.1:$NODE_PORT"
}

#######################################
# Returns a chain era.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_chain_era() {
    local NET_ID=${1}
    local NODE_ID=${2}

    if [ $(get_node_is_up $NET_ID $NODE_ID) = true ]; then
        $(get_path_to_client $NET_ID) get-block \
            --node-address $(get_node_address_rpc $NET_ID $NODE_ID) \
            --block-identifier "" \
            | jq '.result.block.header.era_id'    
    else
        echo "N/A"
    fi
}

#######################################
# Returns a chain height.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_chain_height() {
    local NET_ID=${1}
    local NODE_ID=${2}

    if [ $(get_node_is_up $NET_ID $NODE_ID) = true ]; then
        $(get_path_to_client $NET_ID) get-block \
            --node-address $(get_node_address_rpc $NET_ID $NODE_ID) \
            --block-identifier "" \
            | jq '.result.block.header.height'    
    else
        echo "N/A"
    fi
}

#######################################
# Returns a chain name.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_chain_name() {
    local NET_ID=${1}

    echo casper-net-$NET_ID
}

#######################################
# Returns latest block finalized at a node.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_chain_latest_block_hash() {
    local NET_ID=${1}
    local NODE_ID=${2:-1}
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    $(get_path_to_client $NET_ID) get-block \
        --node-address $NODE_ADDRESS \
        | jq '.result.block.hash' \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns a timestamp for use in chainspec.toml.
# Arguments:
#   Delay in seconds to apply to genesis timestamp.
#######################################
function get_genesis_timestamp()
{
    local DELAY=${1}
    local SCRIPT=(
        "from datetime import datetime, timedelta;"
        "print((datetime.utcnow() + timedelta(seconds=$DELAY)).isoformat('T') + 'Z');"
     )

    python3 -c "${SCRIPT[*]}"
}

#######################################
# Returns a main purse uref.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Account key.
#   State root hash.
#######################################
function get_main_purse_uref() {
    local NET_ID=${1}    
    local NODE_ID=${2}  
    local ACCOUNT_KEY=${3}
    local STATE_ROOT_HASH=${4}

    STATE_ROOT_HASH=${STATE_ROOT_HASH:-$(get_state_root_hash $NET_ID $NODE_ID)}

    echo $(
        source $NCTL/sh/views/view_chain_account.sh \
            net=$NET_ID node=$NODE_ID account-key=$ACCOUNT_KEY root-hash=$STATE_ROOT_HASH \
            | jq '.stored_value.Account.main_purse' \
            | sed -e 's/^"//' -e 's/"$//'
    )
}

#######################################
# Returns network bind address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Network bootstrap count.
#######################################
function get_network_bind_address() {
    local NET_ID=${1}    
    local NODE_ID=${2}   
    local NET_BOOTSTRAP_COUNT=${3}     

    echo "0.0.0.0:$(get_node_port $NCTL_BASE_PORT_NETWORK $NET_ID $NODE_ID)"   
}

#######################################
# Returns network known addresses.
# Arguments:
#   Network ordinal identifier.
#   Network bootstrap count.
#######################################
function get_network_known_addresses() {
    local NET_ID=${1}    
    local NET_BOOTSTRAP_COUNT=${2}    

    local RESULT=""
    local ADDRESS=""
    for IDX in $(seq 1 $NET_BOOTSTRAP_COUNT)
    do
        ADDRESS=$(get_bootstrap_known_address $NET_ID $IDX)
        RESULT=$RESULT$ADDRESS
        if [ $IDX -lt $NET_BOOTSTRAP_COUNT ]; then
            RESULT=$RESULT","
        fi        
    done

    echo $RESULT
}

#######################################
# Returns count of a network's bootstrap nodes.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_count_of_bootstrap_nodes() {
    local NET_ID=${1}    
    source $(get_path_to_net_vars $NET_ID)

    echo $NCTL_NET_BOOTSTRAP_COUNT
}

#######################################
# Returns count of a network's bootstrap nodes.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_count_of_genesis_nodes() {
    local NET_ID=${1}    
    source $(get_path_to_net_vars $NET_ID)

    echo $NCTL_NET_NODE_COUNT
}

#######################################
# Returns count of a network configured nodes.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_count_of_all_nodes() {    
    local NET_ID=${1}

    source $(get_path_to_net_vars $NET_ID)

    echo $(($NCTL_NET_NODE_COUNT * 2))
}

#######################################
# Returns count of test users.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_count_of_users() {    
    local NET_ID=${1}

    source $(get_path_to_net_vars $NET_ID)

    echo $NCTL_NET_USER_COUNT
}

#######################################
# Returns node event address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_event() {
    local NET_ID=${1}    
    local NODE_ID=${2}    

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_SSE $NET_ID $NODE_ID)"
}

#######################################
# Returns node JSON address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rest() {
    local NET_ID=${1}    
    local NODE_ID=${2}   

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_REST $NET_ID $NODE_ID)"
}

#######################################
# Returns node RPC address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rpc() {
    local NET_ID=${1}    
    local NODE_ID=${2}      

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_RPC $NET_ID $NODE_ID)"
}

#######################################
# Returns node RPC address, intended for use with cURL.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rpc_for_curl() {
    local NET_ID=${1}    
    local NODE_ID=${2}   

    # For cURL, need to append '/rpc' to the RPC endpoint URL.
    # This suffix is not needed for use with the client via '--node-address'.
    echo "$(get_node_address_rpc $NET_ID $NODE_ID)/rpc"
}

#######################################
# Calculate port for a given base port, network id, and node id.
# Arguments:
#   Base starting port.
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_port() {
    local BASE_PORT=${1}    
    local NET_ID=${2}    
    local NODE_ID=${3}    

    # TODO: Need to handle case of more than 99 nodes.
    echo $(($BASE_PORT + ($NET_ID * 100) + $NODE_ID))
}

#######################################
# Calculates REST port.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_port_rest() {
    local NET_ID=${1}    
    local NODE_ID=${2}    

    echo $(get_node_port $NCTL_BASE_PORT_REST $NET_ID $NODE_ID)
}

#######################################
# Calculates RPC port.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_port_rpc() {
    local NET_ID=${1}    
    local NODE_ID=${2}    

    echo $(get_node_port $NCTL_BASE_PORT_RPC $NET_ID $NODE_ID)
}

#######################################
# Calculates SSE port.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_port_sse() {
    local NET_ID=${1}    
    local NODE_ID=${2}    

    echo $(get_node_port $NCTL_BASE_PORT_SSE $NET_ID $NODE_ID)
}

function xxx() {
    nmap -p 59105 127.0.0.1 | grep "open"
}

#######################################
# Returns flag indicating whether a node is currently up.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_is_up() {
    local NET_ID=${1}    
    local NODE_ID=${2}    

    local NODE_PORT=$(get_node_port_rest $NET_ID $NODE_ID)

    if grep -q "open" <<< "$(nmap -p $NODE_PORT 127.0.0.1)"; then
        echo true
    else
        echo false
    fi
}

#######################################
# Returns name of a daemonized node process.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_group_members() {
    local NET_ID=${1}    
    local PROCESS_GROUP=${2}

    # Import net vars.
    source $(get_path_to_net_vars $NET_ID)

    # Set range.
    if [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_1 ]; then
        local SEQ_START=1
        local SEQ_END=$NCTL_NET_BOOTSTRAP_COUNT
    elif [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_2 ]; then
        local SEQ_START=$(($NCTL_NET_BOOTSTRAP_COUNT + 1))
        local SEQ_END=$NCTL_NET_NODE_COUNT
    elif [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_3 ]; then
        local SEQ_START=$(($NCTL_NET_NODE_COUNT + 1))
        local SEQ_END=$(($NCTL_NET_NODE_COUNT * 2))
    fi

    # Set members of process group.
    local result=""
    for IDX in $(seq $SEQ_START $SEQ_END)
    do
        if [ $IDX -gt $SEQ_START ]; then
            result=$result", "
        fi
        result=$result$(get_process_name_of_node $NET_ID $IDX)
    done

    echo $result
}

#######################################
# Returns name of a daemonized node process within a group.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_name_of_node() {
    local NET_ID=${1}    
    local NODE_ID=${2} 
    
    echo "casper-net-$NET_ID-node-$NODE_ID"
}

#######################################
# Returns name of a daemonized node process within a group.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_name_of_node_in_group() {
    local NET_ID=${1}    
    local NODE_ID=${2} 
    local NODE_PROCESS_NAME=$(get_process_name_of_node $NET_ID $NODE_ID)
    local PROCESS_GROUP_NAME=$(get_process_name_of_node_group $NET_ID $NODE_ID)
    
    echo "$PROCESS_GROUP_NAME:$NODE_PROCESS_NAME"
}

#######################################
# Returns name of a daemonized node process group.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_name_of_node_group() {
    local NET_ID=${1}    
    local NODE_ID=${2}
    
    source $(get_path_to_net_vars $NET_ID)

    # Boostraps.
    if [ $NODE_ID -le $NCTL_NET_BOOTSTRAP_COUNT ]; then
        echo $NCTL_PROCESS_GROUP_1
    # Genesis validators.
    elif [ $NODE_ID -le $NCTL_NET_NODE_COUNT ]; then
        echo $NCTL_PROCESS_GROUP_2
    # Other.
    else
        echo $NCTL_PROCESS_GROUP_3
    fi
}

#######################################
# Returns path to a binary file.
# Arguments:
#   Network ordinal identifier.
#   Binary file name.
#######################################
function get_path_to_binary() {
    local NET_ID=${1}   
    local FILENAME=${2}    

    echo $(get_path_to_net $NET_ID)/bin/$FILENAME
}

#######################################
# Returns path to client binary.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_client() {
    local NET_ID=${1} 

    echo $(get_path_to_binary $NET_ID "casper-client")
}

#######################################
# Returns path to a smart contract.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Contract wasm file name.
#######################################
function get_path_to_contract() {
    local NET_ID=${1} 
    local FILENAME=${2}

    echo $(get_path_to_binary $NET_ID $FILENAME)
}

#######################################
# Returns path to a network faucet.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_faucet() {
    local NET_ID=${1}   

    echo $(get_path_to_net $NET_ID)/faucet
}

#######################################
# Returns path to a network's assets.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_net() {
    local NET_ID=${1}

    echo $NCTL/assets/net-$NET_ID
}

#######################################
# Returns path to a network's dump folder.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_net_dump() {
    local NET_ID=${1}

    echo $NCTL/dumps/net-$NET_ID
}

#######################################
# Returns path to a network's supervisord config file.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_net_supervisord_cfg() {
    local NET_ID=${1}
    local PATH_TO_NET=$(get_path_to_net $NET_ID)

    echo $PATH_TO_NET/daemon/config/supervisord.conf
}

#######################################
# Returns path to a network's supervisord socket file.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_net_supervisord_sock() {
    local NET_ID=${1}
    local PATH_TO_NET=$(get_path_to_net $NET_ID)

    echo $PATH_TO_NET/daemon/socket/supervisord.sock
}

#######################################
# Returns path to a network's variables.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_net_vars() {
    local NET_ID=${1}

    echo $(get_path_to_net $NET_ID)/vars
}

#######################################
# Returns path to a node's assets.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_path_to_node() {
    local NET_ID=${1}
    local NODE_ID=${2} 

    echo $(get_path_to_net $NET_ID)/nodes/node-$NODE_ID
}

#######################################
# Returns path to a secret key.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function get_path_to_secret_key() {
    local NET_ID=${1}
    local ACCOUNT_TYPE=${2}
    local ACCOUNT_IDX=${3}

    if [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        echo $(get_path_to_faucet $NET_ID)/secret_key.pem
    elif [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_NODE ]; then
        echo $(get_path_to_node $NET_ID $ACCOUNT_IDX)/keys/secret_key.pem
    elif [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_USER ]; then
        echo $(get_path_to_user $NET_ID $ACCOUNT_IDX)/secret_key.pem
    fi
}

#######################################
# Returns path to a user's assets.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.
#######################################
function get_path_to_user() {
    local NET_ID=${1}
    local USER_ID=${2}

    echo $(get_path_to_net $NET_ID)/users/user-$USER_ID
}

#######################################
# Returns a state root hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Block identifier.
#######################################
function get_state_root_hash() {
    local NET_ID=${1}
    local NODE_ID=${2} 
    local BLOCK_HASH=${3}

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    $(get_path_to_client $NET_ID) get-state-root-hash \
        --node-address $NODE_ADDRESS \
        --block-identifier ${BLOCK_HASH:-""} \
        | jq '.result.state_root_hash' \
        | sed -e 's/^"//' -e 's/"$//'
}

# ###############################################################
# UTILS: rendering functions
# ###############################################################

#######################################
# Renders an account.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Dispatch node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account() {
    local NET_ID=${1}
    local NODE_ID=${2}      
    local ACCOUNT_TYPE=${3}
    local ACCOUNT_IDX=${4}   

    local ACCOUNT_KEY=$(get_account_key $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local STATE_ROOT_HASH=$(get_state_root_hash $NET_ID $NODE_ID)

    source $NCTL/sh/views/view_chain_account.sh \
        net=$NET_ID \
        node=$NODE_ID \
        root-hash=$STATE_ROOT_HASH \
        account-key=$ACCOUNT_KEY
}

#######################################
# Renders an account's main purse uref.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_main_purse_uref() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local ACCOUNT_TYPE=${3}
    local ACCOUNT_IDX=${4}  
    local STATE_ROOT_HASH=${5:-$(get_state_root_hash $NET_ID $NODE_ID)}

    local ACCOUNT_KEY=$(get_account_key $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_PREFIX=$(get_account_prefix $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local PURSE_UREF=$(get_main_purse_uref $NET_ID $NODE_ID $ACCOUNT_KEY $STATE_ROOT_HASH)

    log "$ACCOUNT_PREFIX.main-purse-uref = $PURSE_UREF"
}

#######################################
# Renders an account secret key path.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_secret_key() {
    local NET_ID=${1}
    local ACCOUNT_TYPE=${2}
    local ACCOUNT_IDX=${3}    

    local ACCOUNT_PREFIX=$(get_account_prefix $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local PATH_TO_KEY=$(get_path_to_secret_key $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)

    log "$ACCOUNT_PREFIX.secret-key-path = $PATH_TO_KEY"
}

#######################################
# Renders on-chain auction information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_chain_auction_info() {
    local NET_ID=${1}
    local NODE_ID=${2}

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    $(get_path_to_client $NET_ID) get-auction-info \
        --node-address $NODE_ADDRESS \
        | jq '.result'
}

#######################################
# Renders on-chain era information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_chain_era_info() {
    local NET_ID=${1}
    local NODE_ID=${2}

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    $(get_path_to_client $NET_ID) get-era-info \
        --node-address $NODE_ADDRESS \
        --block-identifier "" \
        | jq '.result'
}

#######################################
# Renders on-chain block information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_chain_block() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local BLOCK_HASH=${3}      
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    if [ "$BLOCK_HASH" ]; then
        $(get_path_to_client $NET_ID) get-block \
            --node-address $NODE_ADDRESS \
            --block-identifier $BLOCK_HASH \
            | jq '.result.block'
    else
        $(get_path_to_client $NET_ID) get-block \
            --node-address $NODE_ADDRESS \
            | jq '.result.block'
    fi
}

#######################################
# Renders on-chain deploy information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_chain_deploy() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local DEPLOY_HASH=${3}     

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    $(get_path_to_client $NET_ID) get-deploy \
        --node-address $NODE_ADDRESS \
        $DEPLOY_HASH \
        | jq '.result'
}

#######################################
# Renders a state root hash at a certain node.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.
#######################################
function render_chain_state_root_hash() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local BLOCK_HASH=${3}

    local NODE_IS_UP=$(get_node_is_up $NET_ID $NODE_ID)
    if [ "$NODE_IS_UP" = true ]; then
        local STATE_ROOT_HASH=$(get_state_root_hash $NET_ID $NODE_ID $BLOCK_HASH)
    fi

    log "state root hash @ net-$NET_ID.node-$NODE_ID = "${STATE_ROOT_HASH:-'N/A'}
}

#######################################
# Displays to stdout current node status.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_rpc_schema() {
    local NET_ID=${1}
    local NODE_ID=${2}
    
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl $NET_ID $NODE_ID)

    log "net #$NET_ID :: node #$NODE_ID :: $NODE_ADDRESS :: rpc schema:"

    curl -s --header 'Content-Type: application/json' \
        --request POST $NODE_ADDRESS_CURL \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "rpc.discover"
        }' | jq '.result'
}
