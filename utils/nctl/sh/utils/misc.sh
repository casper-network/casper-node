#!/usr/bin/env bash

# ###############################################################
# UTILS: constants
# ###############################################################

# A type of actor representing a participating node.
NCTL_ACCOUNT_TYPE_FAUCET="faucet"

# A type of actor representing a participating node.
NCTL_ACCOUNT_TYPE_NODE="node"

# A type of actor representing a user.
NCTL_ACCOUNT_TYPE_USER="user"

# Default amount used when making transfers.
NCTL_DEFAULT_TRANSFER_AMOUNT=1000000000   # (1e9)

# Default amount used when making auction bids.
NCTL_DEFAULT_AUCTION_BID_AMOUNT=1000000000   # (1e9)

# Default amount used when delegating.
NCTL_DEFAULT_AUCTION_DELEGATE_AMOUNT=1000000000   # (1e9)

# Default motes to pay for consumed gas.
NCTL_DEFAULT_GAS_PAYMENT=1000000000   # (1e9)

# Default gas price multiplier.
NCTL_DEFAULT_GAS_PRICE=10

# Genesis balance of faucet account.
NCTL_INITIAL_BALANCE_FAUCET=1000000000000000000000000000000000   # (1e33)

# Genesis balance of validator account.
NCTL_INITIAL_BALANCE_VALIDATOR=1000000000000000000000000000000000   # (1e33)

# Base weight applied to a validator at genesis.
NCTL_VALIDATOR_BASE_WEIGHT=1000000000000000   # (1e15)

# Base RPC server port number.
NCTL_BASE_PORT_RPC=40000

# Base JSON server port number.
NCTL_BASE_PORT_REST=50000

# Base event server port number.
NCTL_BASE_PORT_EVENT=60000

# Base network server port number.
NCTL_BASE_PORT_NETWORK=34452

# Set of chain system contracts.
declare -a NCTL_CONTRACTS_SYSTEM=(
    auction_install.wasm
    mint_install.wasm
    pos_install.wasm
    standard_payment_install.wasm
)

# Set of chain system contracts.
declare -a NCTL_CONTRACTS_CLIENT=(
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

# Wraps standard echo by adding application prefix.
function log ()
{
    # Set timestamp.
    declare now=`date +%Y-%m-%dT%H:%M:%S:000000`

    # Support tabs.
    declare tabs=''

    # Emit log message.
    if [ "$1" ]; then
        if [ "$2" ]; then
            for ((i=0; i<$2; i++))
            do
                declare tabs+='\t'
            done
            echo $now" [INFO] [$$] NCTL :: "$tabs$1
        else
            echo $now" [INFO] [$$] NCTL :: "$1
        fi
    else
        echo $now" [INFO] [$$] NCTL :: "
    fi
}

# Wraps standard echo by adding application prefix.
function log_error ()
{
    # Set timestamp.
    declare now=`date +%Y-%m-%dT%H:%M:%S:000000`

    # Support tabs.
    declare tabs=''

    # Emit log message.
    if [ "$1" ]; then
        if [ "$2" ]; then
            for ((i=0; i<$2; i++))
            do
                declare tabs+='\t'
            done
            echo $now" [ERROR] [$$] NCTL :: "$tabs$1
        else
            echo $now" [ERROR] [$$] NCTL :: "$1
        fi
    else
        echo $now" [ERROR] [$$] NCTL :: "
    fi
}

# Wraps pushd command to suppress stdout.
function pushd ()
{
    command pushd "$@" > /dev/null
}

# Wraps popd command to suppress stdout.
function popd ()
{
    command popd "$@" > /dev/null
}

# Forces a directory delete / recreate.
function resetd () {
    dpath=$1
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
    account_pbk=${1:2}
    python3 <<< $(cat <<END
import hashlib;
as_bytes=bytes("ed25519", "utf-8") + bytearray(1) + bytes.fromhex("$account_pbk");
h=hashlib.blake2b(digest_size=32);
h.update(as_bytes);
print(h.digest().hex());
END
    )
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
    if [ $2 = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        path_key=$(get_path_to_faucet $1)/public_key_hex
    elif [ $2 = $NCTL_ACCOUNT_TYPE_NODE ]; then
        path_key=$(get_path_to_node $1 $3)/keys/public_key_hex
    elif [ $2 = $NCTL_ACCOUNT_TYPE_USER ]; then
        path_key=$(get_path_to_user $1 $3)/public_key_hex
    fi
    echo `cat $path_key`  
}

#######################################
# Get network known addresses - i.e. those of bootstrap nodes.
# Arguments:
#   Network ordinal identifier.
#   Count of bootstraps to setup.
#######################################
function get_bootstrap_known_addresses() {
    result=""
    for idx in $(seq 1 $2)
    do
        address=$(get_bootstrap_known_address $1 $idx)
        result=$result$address
        if [ $idx -lt $2 ]; then
            result=$result","
        fi        
    done
    echo $result
}

#######################################
# Get a network known addresses - i.e. those of bootstrap nodes.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_bootstrap_known_address() {
    port=$(($NCTL_BASE_PORT_NETWORK + ($1 * 100) + $2))
    echo "127.0.0.1:$port"
}

#######################################
# Returns a timestamp for use in chainspec.toml.
# Arguments:
#   Delay in seconds to apply to genesis timestamp.
#######################################
function get_genesis_timestamp()
{
    instruction='from datetime import datetime, timedelta; print((datetime.utcnow() + timedelta(seconds='$1')).isoformat("T") + "Z");'
    python3 <<< $instruction
}

#######################################
# Returns a main purse uref.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   State root hash.
#   Accoubnt key.
#######################################
function get_main_purse_uref() {
echo $(
        source $NCTL/sh/views/view_chain_account.sh net=$1 root-hash=$2 account-key=$3 \
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
    if [ $2 -le $3 ]; then
        echo "0.0.0.0:$(get_node_port $NCTL_BASE_PORT_NETWORK $1 $2)"   
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
    if [ $2 -le $3 ]; then
        echo ""
    else
        echo "$(get_bootstrap_known_addresses $1 $3)"
    fi
}

#######################################
# Returns node event address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_event {
    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_EVENT $1 $2)"
}

#######################################
# Returns node JSON address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rest {
    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_REST $1 $2)"
}

#######################################
# Returns node RPC address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rpc {
    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_RPC $1 $2)"
}

#######################################
# Returns node RPC address, intended for use with cURL.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rpc_for_curl {
    # For cURL, need to append '/rpc' to the RPC endpoint URL.
    # This suffix is not needed for use with the client via '--node-address'.
    echo "$(get_node_address_rpc $1 $2)/rpc"
}

#######################################
# Calculate port for a given base port, network id, and node id.
# Arguments:
#   Base starting port.
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_port {
    # TODO: Need to handle case of more than 99 nodes.
    echo $(($1 + ($2 * 100) + $3))
}

#######################################
# Get path to casper client binary.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_client() {
    echo $NCTL/assets/net-$1/bin/casper-client
}

#######################################
# Get path to a casper smart contract.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_contract() {
    echo $NCTL/assets/net-$1/bin/$2
}

#######################################
# Get path to a network faucet.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_faucet() {
    path_net=$(get_path_to_net $1)
    echo $path_net/faucet
}

#######################################
# Get path to a network's assets.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_net() {
    echo $NCTL/assets/net-$1
}

#######################################
# Get path to a network's variables.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_path_to_net_vars() {
    path_net=$(get_path_to_net $1)
    echo $path_net/vars
}

#######################################
# Get path to a node's assets.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_path_to_node() {
    path_net=$(get_path_to_net $1)
    echo $path_net/nodes/node-$2
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
function get_path_to_secret_key() {
    if [ $2 = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        echo $(get_path_to_faucet $1)/secret_key.pem
    elif [ $2 = $NCTL_ACCOUNT_TYPE_NODE ]; then
        echo $(get_path_to_node $1 $3)/keys/secret_key.pem
    elif [ $2 = $NCTL_ACCOUNT_TYPE_USER ]; then
        echo $(get_path_to_user $1 $3)/secret_key.pem
    fi
}

#######################################
# Get path to a user's assets.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.
#######################################
function get_path_to_user() {
    path_net=$(get_path_to_net $1)
    echo $path_net/users/user-$2
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
    node_address=$(get_node_address_rpc $1 $2)
    if [ "$3" ]; then
        $(get_path_to_client $net) get-state-root-hash \
            --node-address $node_address \
            --block-identifier $3 \
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
    state_root_hash=$(get_state_root_hash $1 $2)
    account_key=$(get_account_key $1 $3 $4)
    source $NCTL/sh/views/view_chain_account.sh \
        net=$1 \
        node=$2 \
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
    state_root_hash=$(get_state_root_hash $1 $2)
    account_key=$(get_account_key $1 $3 $4)
    purse_uref=$(get_main_purse_uref $1 $state_root_hash $account_key)
    if [ $3 = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        prefix="net-"$1":faucet"
    elif [ $3 = $NCTL_ACCOUNT_TYPE_NODE ]; then
        prefix="net-"$1":node-"$4
    elif [ $3 = $NCTL_ACCOUNT_TYPE_USER ]; then
        prefix="net-"$1":user-"$4
    fi    
    source $NCTL/sh/views/view_chain_account_balance.sh \
        net=$1 \
        node=$2 \
        root-hash=$state_root_hash \
        purse-uref=$purse_uref \
        prefix=$prefix
}

#######################################
# Renders an account hash.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_hash() {
    account_key=$(get_account_key $1 $2 $3)
    account_hash=$(get_account_hash $account_key)
    if [ $2 = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        log "account hash :: net-$1:faucet -> "$account_hash
    elif [ $2 = $NCTL_ACCOUNT_TYPE_NODE ]; then
        log "account hash :: net-$1:node-$3 -> "$account_hash
    elif [ $2 = $NCTL_ACCOUNT_TYPE_USER ]; then
        log "account hash :: net-$1:user-$3 -> "$account_hash
    fi
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
    account_key=$(get_account_key $1 $2 $3)
    if [ $2 = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        log "account key :: net-$1:faucet -> "$account_key
    elif [ $2 = $NCTL_ACCOUNT_TYPE_NODE ]; then
        log "account key :: net-$1:node-$3 -> "$account_key
    elif [ $2 = $NCTL_ACCOUNT_TYPE_USER ]; then
        log "account key :: net-$1:user-$3 -> "$account_key
    fi
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
    path_key=$(get_path_to_secret_key $1 $2 $3)
    if [ $2 = $NCTL_ACCOUNT_TYPE_FAUCET ]; then
        log "secret key :: net-$1:faucet -> "$path_key
    elif [ $2 = $NCTL_ACCOUNT_TYPE_NODE ]; then
        log "secret key :: net-$1:node-$3 -> "$path_key
    elif [ $2 = $NCTL_ACCOUNT_TYPE_USER ]; then
        log "secret key :: net-$1:user-$3 -> "$path_key
    fi
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
    node_address=$(get_node_address_rpc $1 $2)
    state_root_hash=$(get_state_root_hash $1 $2 $3)
    log "STATE ROOT HASH @ "$node_address" :: "$state_root_hash
}
