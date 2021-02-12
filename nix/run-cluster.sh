#!/bin/sh

set -e

NETNAME=mynet
NAMESPACE=casper-${NETNAME}

# if [ $# -ne 1 ]; then
#   echo "usage: $0 TAG"
#   exit 1;
# fi;

# Create a new network.
rm -rf ${NETNAME}
../utils/casper-tool/casper-tool.py create-network --number-of-nodes 5 ${NETNAME}

# Drop the old namespace.
./demo-cluster.py destroy ${NETNAME}
./demo-cluster.py deploy ${NETNAME}

kubectl --namespace ${NAMESPACE} apply -f casper-network.yaml
