#!/usr/bin/env bash

# ###############################################################
# UTILS: conostants
# ###############################################################

# OS types.
declare _OS_LINUX="linux"
declare _OS_LINUX_REDHAT="$_OS_LINUX-redhat"
declare _OS_LINUX_SUSE="$_OS_LINUX-suse"
declare _OS_LINUX_ARCH="$_OS_LINUX-arch"
declare _OS_LINUX_DEBIAN="$_OS_LINUX-debian"
declare _OS_MACOSX="macosx"
declare _OS_UNKNOWN="unknown"

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
# Returns node address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_address {
    echo http://localhost:"$(get_node_port $1 $2)"
}

#######################################
# Returns node api address.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_api {
    echo $(get_node_address $1 $2)/rpc
}

#######################################
# Returns node port.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_node_port {
    echo $((50000 + ($1 * 100) + $2))
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
# Executes a node RESTful GET call.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   REST endpoint.
#######################################
function exec_node_rest_get() {
    node_api_ep=$(get_node_address $1 $2)/$3
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
    node_api_ep=$(get_node_address $1 $2)/rpc
    curl \
        -s \
        --location \
        --request POST $node_api_ep \
        --header 'Content-Type: application/json' \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "'$3'",
            "params":['$4']
        }' | python3 -m json.tool    
}
