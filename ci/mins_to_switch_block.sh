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

SB_TIMESTAMP=$(casper-client get-block -b $LAST_SWITCH_BLOCK $NODE_ADDRESS | jq -r .result.block_with_signatures.block.Version2.header.timestamp | tr -d "/n")

LAST_ERA_ID=$(casper-client get-block -b $LAST_SWITCH_BLOCK $NODE_ADDRESS | jq -r .result.block_with_signatures.block.Version2.header.era_id | tr -d "/n")

SB_EPOCH=$(date -d "$SB_TIMESTAMP" +%s)
NOW_EPOCH=$(date +%s)

MINS_TILL_SB=$(( 120 - ((NOW_EPOCH - $SB_EPOCH) / 60) ))
NEXT_ERA=$(( LAST_ERA_ID + 1 ))
echo "$MINS_TILL_SB mins till era $NEXT_ERA"
