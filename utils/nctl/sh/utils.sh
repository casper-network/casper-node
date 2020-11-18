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

# Base RPC server port number.
export NCTL_BASE_PORT_RPC=40000

# Base JSON server port number.
export NCTL_BASE_PORT_REST=50000

# Base event server port number.
export NCTL_BASE_PORT_EVENT=60000

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
    local now=`date +%Y-%m-%dT%H:%M:%S:000000`
    local tabs=''

    if [ "$1" ]; then
        if [ "$2" ]; then
            for ((i=0; i<$2; i++))
            do
                tabs+='\t'
            done
            echo $now" [INFO] [$$] NCTL :: "$tabs$1
        else
            echo $now" [INFO] [$$] NCTL :: "$1
        fi
    else
        echo $now" [INFO] [$$] NCTL :: "
    fi
}

#######################################
# Wraps standard echo by adding application error prefix.
#######################################
function log_error ()
{
    local now=`date +%Y-%m-%dT%H:%M:%S:000000`
    local tabs=''

    # Emit log message.
    if [ "$1" ]; then
        if [ "$2" ]; then
            for ((i=0; i<$2; i++))
            do
                tabs+='\t'
            done
            echo $now" [ERROR] [$$] NCTL :: "$tabs$1
        else
            echo $now" [ERROR] [$$] NCTL :: "$1
        fi
    else
        echo $now" [ERROR] [$$] NCTL :: "
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
    local dpath=${1}

    if [ -d $dpath ]; then
        rm -rf $dpath
    fi
    mkdir -p $dpath
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
    local account_key=${1}

    local account_public_key=${account_key:2}
    local SCRIPT=(
        "import hashlib;"
        "as_bytes=bytes('ed25519', 'utf-8') + bytearray(1) + bytes.fromhex('$account_public_key');"
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
    local net=${1}
    local account_type=${2}
    local idx=${3}

    if [ $account_type = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        echo `cat $(get_path_to_faucet $net)/public_key_hex`
    elif [ $account_type = $NCTL_ACCOUNT_TYPE_NODE ]; then
        echo `cat $(get_path_to_node $net $idx)/keys/public_key_hex`
    elif [ $account_type = $NCTL_ACCOUNT_TYPE_USER ]; then
        echo `cat $(get_path_to_user $net $idx)/public_key_hex`
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
function get_account_prefix() 
{
    local net=${1}
    local account_type=${2}

    local prefix="net-$net.$account_type"
    if [ $account_type != $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        prefix=$prefix"-"$account_idx
    fi 

    echo $prefix
}

#######################################
# Returns network known addresses - i.e. those of bootstrap nodes.
# Arguments:
#   Network ordinal identifier.
#   Count of bootstraps to setup.
#######################################
function get_bootstrap_known_addresses() {
    local net=${1}
    local bootstraps=${2}

    local result=""
    for idx in $(seq 1 $bootstraps)
    do
        local address=$(get_bootstrap_known_address $net $idx)
        result=$result$address
        if [ $idx -lt $bootstraps ]; then
            result=$result","
        fi        
    done

    echo $result
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
    local net=${1}
    local node=${2}
    local port=$(($NCTL_BASE_PORT_NETWORK + ($net * 100) + $node))

    echo "127.0.0.1:$port"
}

#######################################
# Returns a chain name.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_chain_name() {
    local net=${1}

    echo casper-net-$net
}

#######################################
# Returns a timestamp for use in chainspec.toml.
# Arguments:
#   Delay in seconds to apply to genesis timestamp.
#######################################
function get_genesis_timestamp()
{
    local delay=${1}
    local SCRIPT=(
        "from datetime import datetime, timedelta;"
        "print((datetime.utcnow() + timedelta(seconds=$delay)).isoformat('T') + 'Z');"
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
    local net=${1}
    local node=${2}
    local account_key=${3}
    local state_root_hash=${4}

    if [ ! "$state_root_hash" ]; then
        state_root_hash=$(get_state_root_hash $net $node)
    fi

    echo $(
        source $NCTL/sh/views/view_chain_account.sh \
            net=$net node=$node account-key=$account_key root-hash=$state_root_hash \
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
function get_network_bind_address {
    local net=${1}    
    local node=${2}    
    local bootstraps=${3}    

    if [ $node -le $bootstraps ]; then
        echo "0.0.0.0:$(get_node_port $NCTL_BASE_PORT_NETWORK $net $node)"   
    else
        echo "0.0.0.0:0"   
    fi
}

#######################################
# Returns network known addresses.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Network bootstrap count.
#######################################
function get_network_known_addresses {
    local net=${1}    
    local node=${2}    
    local bootstraps=${3}    

    if [ $node -le $bootstraps ]; then
        echo ""
    else
        echo "$(get_bootstrap_known_addresses $net $bootstraps)"
    fi
}

#######################################
# Returns node event address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_event {
    local net=${1}    
    local node=${2}    

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_EVENT $net $node)"
}

#######################################
# Returns node JSON address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rest {
    local net=${1}    
    local node=${2}    

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_REST $net $node)"
}

#######################################
# Returns node RPC address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rpc {
    local net=${1}    
    local node=${2}    

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_RPC $net $node)"
}

#######################################
# Returns node RPC address, intended for use with cURL.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rpc_for_curl {
    local net=${1}    
    local node=${2}    

    # For cURL, need to append '/rpc' to the RPC endpoint URL.
    # This suffix is not needed for use with the client via '--node-address'.
    echo "$(get_node_address_rpc $net $node)/rpc"
}

#######################################
# Calculate port for a given base port, network id, and node id.
# Arguments:
#   Base starting port.
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_port {
    local base_port=${1}    
    local net=${2}    
    local node=${3}    

    # TODO: Need to handle case of more than 99 nodes.
    echo $(($base_port + ($net * 100) + $node))
}

#######################################
# Returns path to a binary file.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Binary file name.
#######################################
function get_path_to_binary() {
    local net=${1}    
    local filename=${2}    

    echo $(get_path_to_net $1)/bin/$filename
}

#######################################
# Returns path to client binary.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_client() {
    local net=${1}

    echo $(get_path_to_binary $net "casper-client")
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
    local net=${1}
    local filename=${2}

    echo $(get_path_to_binary $net $filename)
}

#######################################
# Returns path to a network faucet.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_faucet() {
    local net=${1}    

    echo $(get_path_to_net $1)/faucet
}

#######################################
# Returns path to a network's assets.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_net() {
    local net=${1}

    echo $NCTL/assets/net-$net
}

#######################################
# Returns path to a network's variables.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_net_vars() {
    local net=${1}

    echo $(get_path_to_net $net)/vars
}

#######################################
# Returns path to a node's assets.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_path_to_node() {
    local net=${1}
    local node=${2}

    echo $(get_path_to_net $net)/nodes/node-$node
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
    local net=${1}
    local account_type=${2}
    local account_idx=${3}

    if [ $account_type = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        echo $(get_path_to_faucet $net)/secret_key.pem
    elif [ $account_type = $NCTL_ACCOUNT_TYPE_NODE ]; then
        echo $(get_path_to_node $net $account_idx)/keys/secret_key.pem
    elif [ $account_type = $NCTL_ACCOUNT_TYPE_USER ]; then
        echo $(get_path_to_user $net $account_idx)/secret_key.pem
    fi
}

#######################################
# Returns path to a user's assets.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.
#######################################
function get_path_to_user() {
    local net=${1}
    local user_idx=${2}

    echo $(get_path_to_net $net)/users/user-$user_idx
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
    local net=${1}    
    local node=${2}
    local block_hash=${3}
    
    local node_address=$(get_node_address_rpc $net $node)

    if [ "$block_hash" ]; then
        $(get_path_to_client $net) get-state-root-hash \
            --node-address $node_address \
            --block-identifier $block_hash \
            | jq '.result.state_root_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    else
        $(get_path_to_client $net) get-state-root-hash \
            --node-address $node_address \
            | jq '.result.state_root_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    fi
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
    local net=${1}    
    local node=${2}       
    local account_type=${3}       
    local account_idx=${4}    

    local state_root_hash=$(get_state_root_hash $net $node)
    local account_key=$(get_account_key $net $account_type $account_idx)

    source $NCTL/sh/views/view_chain_account.sh \
        net=$net \
        node=$node \
        root-hash=$state_root_hash \
        account-key=$account_key
}

#######################################
# Renders an account balance.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Dispatch node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_balance() {
    local net=${1}    
    local node=${2}       
    local account_type=${3}       
    local account_idx=${4}       
    local account_prefix=$(get_account_prefix $net $account_type $account_idx)
    local account_key=$(get_account_key $net $account_type $account_idx)
    local state_root_hash=$(get_state_root_hash $net $node)
    local purse_uref=$(get_main_purse_uref $net $node $account_key $state_root_hash)

    source $NCTL/sh/views/view_chain_account_balance.sh \
        net=$net \
        node=$node \
        root-hash=$state_root_hash \
        purse-uref=$purse_uref \
        prefix=$account_prefix
}

#######################################
# Renders an account hash.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_hash() {
    local net=${1}    
    local account_type=${2}    
    local account_idx=${3}    
    local account_key=$(get_account_key $net $account_type $account_idx)
    local account_hash=$(get_account_hash $account_key)
    local account_prefix=$(get_account_prefix $net $account_type $account_idx)

    log "$account_prefix.account-hash = $account_hash"
}

#######################################
# Renders an account key.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_key() {
    local net=${1}
    local account_type=${2}
    local account_idx=${3}
    local account_key=$(get_account_key $net $account_type $account_idx)
    local account_prefix=$(get_account_prefix $net $account_type $account_idx)

    log "$account_prefix.account-key = $account_key"
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
    local net=${1}
    local node=${2}
    local account_type=${3}
    local account_idx=${4}

    local account_key=$(get_account_key $net $account_type $account_idx)
    local account_prefix=$(get_account_prefix $net $account_type $account_idx)
    local state_root_hash=${5:-$(get_state_root_hash $net $node)}
    local purse_uref=$(get_main_purse_uref $net $node $account_key $state_root_hash)

    log "$account_prefix.main-purse-uref = $purse_uref"
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
    local net=${1}
    local account_type=${2}
    local account_idx=${3}    

    local account_prefix=$(get_account_prefix $net $account_type $account_idx)
    local path_key=$(get_path_to_secret_key $net $account_type $account_idx)

    log "$account_prefix.secret-key-path = $path_key"
}

#######################################
# Renders on-chain auction information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_chain_auction_info() {
    local net=${1}    
    local node=${2}       

    local node_address=$(get_node_address_rpc $net $node)

    $(get_path_to_client $net) get-auction-info \
        --node-address $node_address \
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
    local net=${1}    
    local node=${2}       
    local block_hash=${3}       

    local node_address=$(get_node_address_rpc $net $node)

    if [ "$block_hash" ]; then
        $(get_path_to_client $net) get-block \
            --node-address $node_address \
            --block-identifier $block_hash \
            | jq '.result.block'
    else
        $(get_path_to_client $net) get-block \
            --node-address $node_address \
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
    local net=${1}    
    local node=${2}       
    local deploy_hash=${3}       

    local node_address=$(get_node_address_rpc $net $node)

    # Pull on-chain deploy representation & pull.
    $(get_path_to_client $net) get-deploy \
        --node-address $node_address \
        $deploy_hash \
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
    local net=${1}
    local node=${2}
    local block_hash=${3}
    local node_address=$(get_node_address_rpc $net $node)
    local state_root_hash=$(get_state_root_hash $net $node $block_hash)

    log "STATE ROOT HASH @ "$node_address" :: "$state_root_hash
}

#######################################
# Displays to stdout current node peers.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_peers() {
    local net=${1}
    local node=${2}

    local node_address=$(get_node_address_rpc $net $node)
    local node_address_curl=$(get_node_address_rpc_for_curl $net $node)

    log "network #$net :: node #$2 :: $node_address :: peers:"
    curl -s --header 'Content-Type: application/json' \
        --request POST $node_address_curl \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "info_get_peers"
        }' | jq '.result.peers'
}

#######################################
# Displays to stdout current node status.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_status() {
    local net=${1}
    local node=${2}

    local node_address=$(get_node_address_rpc $net $node)
    local node_address_curl=$(get_node_address_rpc_for_curl $net $node)

    log "network #$net :: node #$2 :: $node_address :: status:"
    curl -s --header 'Content-Type: application/json' \
        --request POST $node_address_curl \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "info_get_status"
        }' | jq '.result'
}

#######################################
# Displays to stdout current node storage stats.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_storage() {
    local net=${1}
    local node=${2}
    
    local os_type="$(get_os)"
    local path_node_storage=$NCTL/assets/net-$net/nodes/node-$node/storage/*.*db*

    log "network #$net :: node #$node :: storage stats:"
    if [[ $os_type == $_OS_LINUX* ]]; then
        ll $path_node_storage
    elif [[ $os_type == $_OS_MACOSX ]]; then
        ls -lG $path_node_storage
    fi
}
