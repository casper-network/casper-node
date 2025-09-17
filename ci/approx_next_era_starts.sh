#!/usr/bin/env bash

if [ "$#" -le 1 ]; then
  echo "Usage: $0 <node ip for rpc> [number of future eras]"
  exit 1
fi

NODE_IP=$1

if ! command -v "casper-client" &> /dev/null ; then
  echo "casper-client is not installed and required. Exiting..."
  exit 1 
fi

if [ "$#" -lt 2 ]; then
  echo "No number of future eras given, using default."
  FUTURE_ERAS=10
else
  FUTURE_ERAS=$2
fi

NODE_ADDRESS="--node-address http://$NODE_IP:7777"

LAST_SWITCH_BLOCK=$(casper-client get-era-summary $NODE_ADDRESS | jq -r .result.era_summary.block_hash | tr -d "/n")

SB_TIMESTAMP=$(casper-client get-block -b $LAST_SWITCH_BLOCK $NODE_ADDRESS | jq -r .result.block_with_signatures.block.Version2.header.timestamp | tr -d "/n")
SB_EPOCH=$(date -d "$SB_TIMESTAMP" +%s)

LAST_ERA_ID=$(casper-client get-block -b $LAST_SWITCH_BLOCK $NODE_ADDRESS | jq -r .result.block_with_signatures.block.Version2.header.era_id | tr -d "/n")
START_ERA_ID=$(( LAST_ERA_ID + 1 ))
NEXT_ERA_ID=$(( START_ERA_ID + 1 ))

FINAL_ERA_ID=$(( START_ERA_ID + FUTURE_ERAS ))

ERA_TIME_SECONDS=$(( 120*60+9 ))

#echo "current_era:$START_ERA_ID started_utc:$SB_TIMESTAMP"

while (( NEXT_ERA_ID <= FINAL_ERA_ID )); do
  TIMESTAMP_FROM_Z=$(date -u -d "@$SB_EPOCH" +"%Y-%m-%dT%H:%M:%SZ")
  TIMESTAMP_FROM_L=$(date -d "@$SB_EPOCH" +"%Y-%m-%dT%H:%M:%S%z")
  echo "era:$NEXT_ERA_ID utc:$TIMESTAMP_FROM_Z local:$TIMESTAMP_FROM_L"
  let NEXT_ERA_ID++
  SB_EPOCH=$(( SB_EPOCH + ERA_TIME_SECONDS ))
done

