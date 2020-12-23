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
export NCTL_DEFAULT_AUCTION_BID_AMOUNT=1000000000000000   # (1e15)

# Default amount used when delegating.
export NCTL_DEFAULT_AUCTION_DELEGATE_AMOUNT=1000000000   # (1e9)

# Default motes to pay for consumed gas.
export NCTL_DEFAULT_GAS_PAYMENT=10000000000   # (1e10)

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
            echo -e "$NOW [INFO] [$$] NCTL :: $TABS$MSG"
        else
            echo -e "$NOW [INFO] [$$] NCTL :: $MSG"
        fi
    else
        echo -e "$NOW [INFO] [$$] NCTL :: "
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
function resetd ()
{
    local DPATH=${1}

    if [ -d $DPATH ]; then
        rm -rf $DPATH
    fi
    mkdir -p $DPATH
}

# ###############################################################
# UTILS: await functions
# ###############################################################

#######################################
# Awaits for the chain to proceed N eras.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Future era offset to apply.
#######################################
function await_n_eras()
{
    local OFFSET=${1}
    local EMIT_LOG=${2:-false}

    local CURRENT=$(get_chain_era)
    local FUTURE=$(($CURRENT + $OFFSET))

    while [ $CURRENT -lt $FUTURE ];
    do
        if [ $EMIT_LOG = true ]; then
            log "current era = $CURRENT :: future era = $FUTURE ... sleeping 10 seconds"
        fi
        sleep 10.0
        CURRENT=$(get_chain_era)
    done

    if [ $EMIT_LOG = true ]; then
        log "current era = $CURRENT"
    fi
}

#######################################
# Awaits for the chain to proceed N blocks.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Future block height offset to apply.
#######################################
function await_n_blocks()
{
    local OFFSET=${1}
    local EMIT_LOG=${2:-false}

    local CURRENT=$(get_chain_height)
    local FUTURE=$(($CURRENT + $OFFSET))

    while [ $CURRENT -lt $FUTURE ];
    do
        if [ $EMIT_LOG = true ]; then
            log "current block height = $CURRENT :: future height = $FUTURE ... sleeping 2 seconds"
        fi
        sleep 2.0
        CURRENT=$(get_chain_height)
    done

    if [ $EMIT_LOG = true ]; then
        log "current block height = $CURRENT"
    fi
}

#######################################
# Awaits for the chain to proceed N eras.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Future era offset to apply.
#######################################
function await_until_era_n()
{
    local ERA=${1}

    while [ $ERA -lt $(get_chain_era) ];
    do
        sleep 10.0
    done
}

#######################################
# Awaits for the chain to proceed N blocks.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Future block offset to apply.
#######################################
function await_until_block_n()
{
    local HEIGHT=${1}

    while [ $HEIGHT -lt $(get_chain_height) ];
    do
        sleep 10.0
    done
}

# ###############################################################
# UTILS: getter functions
# ###############################################################

#######################################
# Returns an on-chain account hash.
# Arguments:
#   Data to be hashed.
#######################################
function get_account_hash()
{
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
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function get_account_key()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2}

    if [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        echo `cat $(get_path_to_faucet)/public_key_hex`
    elif [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_NODE ]; then
        echo `cat $(get_path_to_node $ACCOUNT_IDX)/keys/public_key_hex`
    elif [ $ACCOUNT_TYPE = $NCTL_ACCOUNT_TYPE_USER ]; then
        echo `cat $(get_path_to_user $ACCOUNT_IDX)/public_key_hex`
    fi
}

#######################################
# Returns an account prefix used when logging.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
# Arguments:
#   Account type.
#   Account index (optional).
#######################################
function get_account_prefix()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2:-}
    local NET_ID=${NET_ID:-1}

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
function get_bootstrap_known_address()
{
    local NODE_ID=${1}
    local NET_ID=${NET_ID:-1}    
    local NODE_PORT=$(($NCTL_BASE_PORT_NETWORK + ($NET_ID * 100) + $NODE_ID))

    echo "127.0.0.1:$NODE_PORT"
}

#######################################
# Returns a chain era.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_chain_era()
{
    local NODE_ID=${1:-$(get_node_for_dispatch)}
    local NODE_ADDRESS=$(get_node_address_rpc $NODE_ID)

    if [ $(get_node_is_up $NODE_ID) = true ]; then
        $(get_path_to_client) get-block \
            --node-address $NODE_ADDRESS \
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
function get_chain_height()
{
    local NODE_ID=${1:-$(get_node_for_dispatch)}
    local NODE_ADDRESS=$(get_node_address_rpc $NODE_ID)

    if [ $(get_node_is_up $NODE_ID) = true ]; then
        $(get_path_to_client) get-block \
            --node-address $NODE_ADDRESS \
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
function get_chain_name()
{
    local NET_ID=${NET_ID:-1}

    echo casper-net-$NET_ID
}

#######################################
# Returns latest block finalized at a node.
#######################################
function get_chain_latest_block_hash()
{
    local NODE_ADDRESS=$(get_node_address_rpc)

    $(get_path_to_client) get-block \
        --node-address $NODE_ADDRESS \
        | jq '.result.block.hash' \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns count of a network's bootstrap nodes.
#######################################
function get_count_of_bootstrap_nodes()
{
    # Hard-coded.
    echo 3
}

#######################################
# Returns count of a network's bootstrap nodes.
#######################################
function get_count_of_genesis_nodes()
{
    echo $(($(get_count_of_nodes) / 2))
}

#######################################
# Returns count of all network nodes.
#######################################
function get_count_of_nodes()
{    
    echo $(ls -l $(get_path_to_net)/nodes | grep -c ^d)
}

#######################################
# Returns count of test users.
#######################################
function get_count_of_users()
{    
    echo $(ls -l $(get_path_to_net)/users | grep -c ^d)
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
#   Account key.
#   State root hash.
#######################################
function get_main_purse_uref()
{
    local ACCOUNT_KEY=${1}
    local STATE_ROOT_HASH=${2:-$(get_state_root_hash)}

    echo $(
        source $NCTL/sh/views/view_chain_account.sh \
            account-key=$ACCOUNT_KEY \
            root-hash=$STATE_ROOT_HASH \
            | jq '.stored_value.Account.main_purse' \
            | sed -e 's/^"//' -e 's/"$//'
    )
}

#######################################
# Returns network bind address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_network_bind_address()
{
    local NODE_ID=${1}

    echo "0.0.0.0:$(get_node_port $NCTL_BASE_PORT_NETWORK $NODE_ID)"   
}

#######################################
# Returns network known addresses.
#######################################
function get_network_known_addresses()
{
    local RESULT=""
    local ADDRESS=""
    for NODE_ID in $(seq 1 $(get_count_of_bootstrap_nodes))
    do
        ADDRESS=$(get_bootstrap_known_address $NODE_ID)
        RESULT=$RESULT$ADDRESS
        if [ $NODE_ID -lt $(get_count_of_bootstrap_nodes) ]; then
            RESULT=$RESULT","
        fi
    done

    echo $RESULT
}

#######################################
# Returns node event address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_event()
{
    local NODE_ID=${1}    

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_SSE $NODE_ID)"
}

#######################################
# Returns node JSON address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rest()
{
    local NODE_ID=${1}   

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_REST $NODE_ID)"
}

#######################################
# Returns node RPC address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rpc()
{
    local NODE_ID=${1}      

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_RPC $NODE_ID)"
}

#######################################
# Returns node RPC address, intended for use with cURL.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rpc_for_curl()
{
    local NODE_ID=${1}   

    # For cURL, need to append '/rpc' to the RPC endpoint URL.
    # This suffix is not needed for use with the client via '--node-address'.
    echo "$(get_node_address_rpc $NODE_ID)/rpc"
}

#######################################
# Returns ordinal identifier of a validator node able to be used for deploy dispatch.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_node_for_dispatch()
{
    local NET_ID=${NET_ID:-1}
    local NODESET=$(seq 1 $(get_count_of_nodes) | shuf)

    for NODE_ID in $NODESET
    do
        if [ $(get_node_is_up $NODE_ID) = true ]; then
            echo $NODE_ID
            break
        fi
    done
}

#######################################
# Calculate port for a given base port, network id, and node id.
# Arguments:
#   Base starting port.
#   Node ordinal identifier.
#######################################
function get_node_port()
{
    local BASE_PORT=${1}    
    local NODE_ID=${2:-$(get_node_for_dispatch)}    
    local NET_ID=${NET_ID:-1}    

    # TODO: Need to handle case of more than 99 nodes.
    echo $(($BASE_PORT + ($NET_ID * 100) + $NODE_ID))
}

#######################################
# Calculates REST port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_rest()
{
    local NODE_ID=${1}    

    echo $(get_node_port $NCTL_BASE_PORT_REST $NODE_ID)
}

#######################################
# Calculates RPC port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_rpc()
{
    local NODE_ID=${1}    

    echo $(get_node_port $NCTL_BASE_PORT_RPC $NODE_ID)
}

#######################################
# Calculates SSE port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_sse()
{
    local NODE_ID=${1}    

    echo $(get_node_port $NCTL_BASE_PORT_SSE $NODE_ID)
}

#######################################
# Returns flag indicating whether a node is currently up.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_is_up()
{
    local NODE_ID=${1}    
    local NODE_PORT=$(get_node_port_rpc $NODE_ID)

    if grep -q "open" <<< "$(nmap -p $NODE_PORT 127.0.0.1)"; then
        echo true
    else
        echo false
    fi
}

#######################################
# Returns set of nodes within a process group.
# Arguments:
#   Process group identifier.
#######################################
function get_process_group_members()
{
    local PROCESS_GROUP=${1}

    # Set range.
    if [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_1 ]; then
        local SEQ_START=1
        local SEQ_END=$(get_count_of_bootstrap_nodes)
    elif [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_2 ]; then
        local SEQ_START=$(($(get_count_of_bootstrap_nodes) + 1))
        local SEQ_END=$(get_count_of_genesis_nodes)
    elif [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_3 ]; then
        local SEQ_START=$(($(get_count_of_genesis_nodes) + 1))
        local SEQ_END=$(get_count_of_nodes)
    fi

    # Set members of process group.
    local result=""
    for NODE_ID in $(seq $SEQ_START $SEQ_END)
    do
        if [ $NODE_ID -gt $SEQ_START ]; then
            result=$result", "
        fi
        result=$result$(get_process_name_of_node $NODE_ID)
    done

    echo $result
}

#######################################
# Returns name of a daemonized node process within a group.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_name_of_node()
{
    local NODE_ID=${1} 
    local NET_ID=${NET_ID:-1}    
    
    echo "casper-net-$NET_ID-node-$NODE_ID"
}

#######################################
# Returns name of a daemonized node process within a group.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_process_name_of_node_in_group()
{
    local NODE_ID=${1} 

    local NODE_PROCESS_NAME=$(get_process_name_of_node $NODE_ID)
    local PROCESS_GROUP_NAME=$(get_process_name_of_node_group $NODE_ID)
    
    echo "$PROCESS_GROUP_NAME:$NODE_PROCESS_NAME"
}

#######################################
# Returns name of a daemonized node process group.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_name_of_node_group()
{
    local NODE_ID=${1} 
    
    if [ $NODE_ID -le $(get_count_of_bootstrap_nodes) ]; then
        echo $NCTL_PROCESS_GROUP_1
    elif [ $NODE_ID -le $(get_count_of_genesis_nodes) ]; then
        echo $NCTL_PROCESS_GROUP_2
    else
        echo $NCTL_PROCESS_GROUP_3
    fi
}

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

#######################################
# Returns a formatted session argument.
# Arguments:
#   Argument name.
#   Argument value.
#   CL type suffix to apply to argument type.
#   CL type prefix to apply to argument value.
#######################################
function get_cl_arg()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}
    local CL_TYPE_SUFFIX=${3}
    local CL_VALUE_PREFIX=${4:-""}

    echo "$ARG_NAME:$CL_TYPE_SUFFIX='$CL_VALUE_PREFIX$ARG_VALUE'"
}

#######################################
# Returns a formatted session argument (cl type=account hash).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_account_hash()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "account_hash" "account-hash-")
}

#######################################
# Returns a formatted session argument (cl type=account key).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_account_key()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "public_key")
}

#######################################
# Returns a formatted session argument (cl type=optional uref).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_opt_uref()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "opt_uref")
}

#######################################
# Returns a formatted session argument (cl type=string).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_string()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "string")
}

#######################################
# Returns a formatted session argument (cl type=u64).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_u64()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "U64")
}

#######################################
# Returns a formatted session argument (cl type=u256).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_u256()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "U256")
}

#######################################
# Returns a formatted session argument (cl type=u512).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_u512()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "U512")
}

#######################################
# Returns a state root hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Node ordinal identifier.
#   Block identifier.
#######################################
function get_state_root_hash()
{
    local NODE_ID=${1} 
    local BLOCK_HASH=${2}

    local NODE_ADDRESS=$(get_node_address_rpc $NODE_ID)

    $(get_path_to_client) get-state-root-hash \
        --node-address $NODE_ADDRESS \
        --block-identifier ${BLOCK_HASH:-""} \
        | jq '.result.state_root_hash' \
        | sed -e 's/^"//' -e 's/"$//'
}
