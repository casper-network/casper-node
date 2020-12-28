#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Prepares native transfers for dispatch to a test net.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Transfer dispatch interval.
#######################################
function main()
{
    echo "prepare native not implemented - awaiting core dev feedback"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset AMOUNT
unset BATCH_COUNT
unset BATCH_SIZE

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        count) BATCH_COUNT=${VALUE} ;;
        size) BATCH_SIZE=${VALUE} ;;
        *)
    esac
done

main "${AMOUNT:-$NCTL_DEFAULT_TRANSFER_AMOUNT}" \
     "${BATCH_COUNT:-5}" \
     "${BATCH_SIZE:-200}" 
