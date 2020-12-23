#######################################
# Imports
#######################################

source $NCTL/sh/utils.sh

#######################################
# Sets entry in chainspec's accounts.csv.
# Arguments:
#   Path to network directory.
#   Path to file containing an ed25519 public key in hex format.
#   Initial account balance (in motes).
#   Staking weight - validator's only.
#######################################
function setup_chainspec_account()
{
    local PATH_TO_NET=${1}
    local PATH_TO_ACCOUNT_KEY=${2}
    local INITIAL_BALANCE=${3}
    local INITIAL_WEIGHT=${4}

    account_key=`cat $PATH_TO_ACCOUNT_KEY`
	cat >> $PATH_TO_NET/chainspec/accounts.csv <<- EOM
	${account_key},$INITIAL_BALANCE,$INITIAL_WEIGHT
	EOM
}

#######################################
# Sets assets pertaining to a single node.
# Globals:
#   NCTL_CASPER_HOME - path to node software github repo.
#   NCTL_VALIDATOR_BASE_WEIGHT - base weight applied to validator POS.
#   NCTL_INITIAL_BALANCE_VALIDATOR - initial balance of a lucky validator.
#   NCTL_ACCOUNT_TYPE_NODE - node account type enum.
# Arguments:
#   Node ordinal identifier.
#   Count of genesis nodes to initially setup.
#######################################
function setup_node()
{
    local NODE_ID=${1}
    local COUNT_GENESIS_NODES=${2}

    local PATH_TO_NET=$(get_path_to_net)
    local PATH_TO_NODE=$(get_path_to_node $NODE_ID)

    # Set directory.
    mkdir $PATH_TO_NODE
    mkdir $PATH_TO_NODE/config
    mkdir $PATH_TO_NODE/keys
    mkdir $PATH_TO_NODE/logs
    mkdir $PATH_TO_NODE/storage

    # Set config.
    cp $NCTL_CASPER_HOME/resources/local/config.toml $PATH_TO_NODE/config/node-config.toml

    # Set keys.
    $(get_path_to_client) keygen -f $PATH_TO_NODE/keys > /dev/null 2>&1

    # Set staking weight.
    local POS_WEIGHT
    if [ $NODE_ID -le $COUNT_GENESIS_NODES ]; then
        POS_WEIGHT=$(($NCTL_VALIDATOR_BASE_WEIGHT * $NODE_ID))
    else
        POS_WEIGHT=0
    fi 

    # Set chainspec account.
	cat >> $PATH_TO_NET/chainspec/accounts.csv <<- EOM
	$(get_account_key $NCTL_ACCOUNT_TYPE_NODE $NODE_ID),$NCTL_INITIAL_BALANCE_VALIDATOR,$POS_WEIGHT
	EOM
}
