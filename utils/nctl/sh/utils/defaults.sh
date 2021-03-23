#!/usr/bin/env bash

# Set default type of daemon to run.
export NCTL_DAEMON_TYPE=${NCTL_DAEMON_TYPE:-supervisord}

# Set default compilation target.
export NCTL_COMPILE_TARGET=${NCTL_COMPILE_TARGET:-release}

# Set default logging output format.
export NCTL_NODE_LOG_FORMAT=${NCTL_NODE_LOG_FORMAT:-json}
