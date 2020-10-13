#!/usr/bin/env bash
#
# Renders node metrics to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        deploy) deploy=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
node=${node:-1}

#######################################
# Main
#######################################


# curl \
#     -s \
#     --location \
#     --request POST $node_api \
#     --header 'Content-Type: application/json' \
#     --data-raw '{
#         "id": 1,
#         "jsonrpc": "2.0",
#         "method": "info_get_deploy",
#         "params": [{
#             "deploy_hash": "'$deploy'"
#         }]
#     }' | python3 -m json.tool  


node_api=$(get_node_api $net $node)

curl \
    -s \
    --location \
    --request POST $node_api \
    --header 'Content-Type: application/json' \
    --data-raw '{
    "id": 1,
    "jsonrpc": "2.0",
    "method": "info_get_deploy",
    "params": [
        {
            "deploy_hash": "91a811fa30881f0baaabb417d7f840d5eff56b125bbb863a39afe2d6b4088e7c"
        }
    ]
}' | echo  

curl \
    -s \
    --location \
    --request POST 'localhost:50101/rpc' \
    --header 'Content-Type: application/json' \
    --data-raw '{
        "id": 1,
        "jsonrpc": "2.0",
        "method": "info_get_deploy",
        "params": [
            {
                "deploy_hash": "91a811fa30881f0baaabb417d7f840d5eff56b125bbb863a39afe2d6b4088e7c"
            }
        ]
    }' | echo