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

# Genesis balance of faucet account.
NCTL_INITIAL_BALANCE_FAUCET=1000000000000000000000000

# Genesis balance of validator account.
NCTL_INITIAL_BALANCE_VALIDATOR=1000000000000000000000000

# Base weight applied to a validator at genesis.
NCTL_VALIDATOR_BASE_WEIGHT=100000000000000

# DEPRECATED: Base HTTP server port number.
NCTL_BASE_PORT_HTTP=50000

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
    instruction='import hashlib; as_bytes=bytes("ed25519", "utf-8") + bytearray(1) + bytes.fromhex("'$account_pbk'"); h=hashlib.blake2b(digest_size=32); h.update(as_bytes); print(h.digest().hex());'
    python3 <<< $instruction
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
# Returns blake2b hash.
# Arguments:
#   Data to be hashed.
#######################################
function get_hash() {
    instruction='import hashlib; h=hashlib.blake2b(digest_size=32); h.update(b"'$1'"); print(h.digest().hex());'
    python3 <<< $instruction
}

#######################################
# Returns prettified JSON from a string.
# Arguments:
#   JSON string to be prettified.
#######################################
function get_json_s() {
    echo "$1" | python3 -m json.tool
}

#######################################
# Returns prettified JSON from a file.
# Arguments:
#   JSON file to be prettified.
#######################################
function get_json_f() {
    python3 -m json.tool "$1"
}

#######################################
# Returns prettified JSON from internet.
# Arguments:
#   URL to JSON to be prettified.
#######################################
function get_json_w() {
    curl "$1" | python3 -m json.tool
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

#######################################
# Executes a node RPC call.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   RPC method.
#   RPC method parameters.
#######################################
function exec_node_rpc() {
    curl \
        -s \
        --location \
        --request POST $(get_node_address_rpc $1 $2) \
        --header 'Content-Type: application/json' \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "'$3'",
            "params": {'$4'}
        }' | jq $5
}
