#!/usr/bin/env bash

if [ "$#" -ne 1 ]; then
  echo "Usage: $0 <node ip for rpc>"
  exit 1
fi

if ! command -v "casper-client" &> /dev/null ; then
  echo "casper-client is not installed and required. Exiting..."
  exit 1 
fi

NODE_IP=$1

NODE_ADDRESS="--node-address http://$NODE_IP:7777"

LAST_SWITCH_BLOCK=$(casper-client get-era-summary $NODE_ADDRESS | jq -r .result.era_summary.block_hash | tr -d "/n")

# Getting Timestamp and Era with one call using `@` delimiter
SB_TIMESTAMP_AND_ERA=$(casper-client get-block -b $LAST_SWITCH_BLOCK $NODE_ADDRESS | jq -r '.result.block_with_signatures.block.Version2.header | [.timestamp,.era_id] | join("@")' | tr -d "/n")

# Parsing this back into seperate variables
IFS=@ read -r SB_TIMESTAMP LAST_ERA_ID <<< "$SB_TIMESTAMP_AND_ERA"

# Converting timestamp into Unix second based Epoch
SB_EPOCH=$(date -d "$SB_TIMESTAMP" +%s)
NOW_EPOCH=$(date +%s)

# Assuming Era length 120 minutes, doing math till next Era
MINS_TILL_SB=$(( 120 - ((NOW_EPOCH - $SB_EPOCH) / 60) ))
NEXT_ERA=$(( LAST_ERA_ID + 1 ))
echo "$MINS_TILL_SB mins till era $NEXT_ERA"
