#!/usr/bin/env bash

# ###############################################################
# UTILS: constants
# ###############################################################

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
# Get network known addresses - i.e. those of bootstrap nodes.
# Arguments:
#   Network ordinal identifer.
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
#   Network ordinal identifer.
#   Node ordinal identifer.
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
