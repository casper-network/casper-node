#!/usr/bin/env bash


# Images used in this script are build in CasperLabs/buildenv repo

# This allows make commands without local build environment setup or
# using an OS version other than locally installed.

set -e

#docker pull casperlabs/node-build-u1804:latest

# Getting user and group to chown/chgrp target folder from root at end.
# Cannot use the --user trick as cached .cargo in image is owned by root.
command="cd /casper-node/client; cargo rpm build; chown -R -f $(id -u):$(id -g) ./target "
docker run --rm --volume $(pwd)/..:/casper-node cent8package:latest /bin/bash -c "${command}"
