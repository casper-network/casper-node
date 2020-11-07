#!/usr/bin/env bash

# ###############################################################
# UTILS: constants
# ###############################################################

# OS types.
declare _OS_LINUX="linux"
declare _OS_LINUX_REDHAT="$_OS_LINUX-redhat"
declare _OS_LINUX_SUSE="$_OS_LINUX-suse"
declare _OS_LINUX_ARCH="$_OS_LINUX-arch"
declare _OS_LINUX_DEBIAN="$_OS_LINUX-debian"
declare _OS_MACOSX="macosx"
declare _OS_UNKNOWN="unknown"

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
NCTL_BASE_PORT_JSON=50000

# Base event server port number.
NCTL_BASE_PORT_EVENT=60000

# Base network server port number.
NCTL_BASE_PORT_NETWORK=34452

# ###############################################################
# UTILS: helper functions
# ###############################################################

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
# Returns node RPC address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_rpc {
    echo http://localhost:"$(calculate_node_port $NCTL_BASE_PORT_RPC $1 $2)"
}

#######################################
# Returns node RPC address, intended for use with cURL.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_curl_node_address_rpc {
    # For cURL, need to append '/rpc' to the RPC endpoint URL.
    # This suffix is not needed for use with the client via '--node-address'.
    echo "$(get_node_address_rpc $1 $2)/rpc"
}

#######################################
# Returns node JSON address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_json {
    echo http://localhost:"$(calculate_node_port $NCTL_BASE_PORT_JSON $1 $2)"
}

#######################################
# Returns node event address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address_event {
    echo http://localhost:"$(calculate_node_port $NCTL_BASE_PORT_EVENT $1 $2)"
}

#######################################
# Calculate port for a given base port, network id, and node id.
# Arguments:
#   Base starting port.
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function calculate_node_port {
    # TODO: Need to handle case of more than 99 nodes.
    echo $(($1 + ($2 * 100) + $3))
}

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
# Executes a node RESTful GET call.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   REST endpoint.
#######################################
function exec_node_rest_get() {
    node_api_ep=$(get_node_address_json $1 $2)/$3
    log $node_api
    curl \
        -s \
        --location \
        --request GET $node_api_ep
}
