#!/usr/bin/env bash

unset NET_ID
unset USER_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        user) USER_ID=${VALUE} ;;
        *)
    esac
done

NET_ID=${NET_ID:-1}
USER_ID=${USER_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh
source $NCTL/sh/views/funcs.sh

if [ $USER_ID = "all" ]; then
    for USER_ID in $(seq 1 $(get_count_of_users $NET_ID))
    do
        render_account_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $USER_ID
    done
else
    render_account_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $USER_ID
fi
