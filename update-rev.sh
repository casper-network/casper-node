#!/bin/bash
set -ue 

DEST_FILE='node/Cargo.toml'

if [ -f $DEST_FILE ]; then 
  echo "[INFO] Going to update $DEST_FILE with revision set to $DRONE_BUILD_NUMBER"
  sed -i s'/^revision =.*/revision = "'"${DRONE_BUILD_NUMBER}"'"/' $DEST_FILE
  cat $DEST_FILE
else
  echo "[ERRPR] Unable to find: $DEST_FILE"
  exit 1
fi
