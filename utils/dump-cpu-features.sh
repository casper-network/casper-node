#!/bin/sh

# Dumps a list of X86 extensions found to be in use by the given binary, in alphabetical order.

set -e

if [ $# -ne 1 ]; then
  echo "usage: $(basename $0) binary"
fi;

BINARY=$1

export PATH="$HOME/.cargo/bin:$PATH"

elfx86exts $BINARY | grep -v 'CPU Generation' | cut -f1 -d ' ' | sort
