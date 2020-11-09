#!/bin/bash
set -ue 

DEST_FILES=("node/Cargo.toml" "client/Cargo.toml")

for f in "${DEST_FILES[@]}"; do
  if [ -f $f ]; then 
    echo "[INFO] Going to update $f with revision set to $DRONE_BUILD_NUMBER"
    sed -i s'/^revision =.*/revision = "'"${DRONE_BUILD_NUMBER}"'"/' $f
    cat $f | grep -i revision
  else
    echo "[ERRPR] Unable to find: $f"
    exit 1
  fi
done
