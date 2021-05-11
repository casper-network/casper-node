#!/usr/bin/env bash

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
	local NOW

    NOW=$(date +%Y-%m-%dT%H:%M:%S.%6N)

    echo -e "$NOW [INFO] [$$] NCTL :: $MSG"
}

#######################################
# Wraps standard echo by adding application error prefix.
#######################################
function log_error ()
{
    local MSG=${1}
    local NOW
	
    NOW=$(date +%Y-%m-%dT%H:%M:%S.%6N)

    echo -e "$NOW [ERROR] [$$] NCTL :: $MSG"
}

#######################################
# Step logging helper..
#######################################
function log_step() 
{
    local STEP_ID=${1}
    local MSG=${2}

    log "Step $STEP_ID: $MSG"
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

    if [ -d "$DPATH" ]; then
        rm -rf "$DPATH"
    fi
    mkdir -p "$DPATH"
}
