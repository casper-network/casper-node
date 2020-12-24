#######################################
# Returns a formatted session argument.
# Arguments:
#   Argument name.
#   Argument value.
#   CL type suffix to apply to argument type.
#   CL type prefix to apply to argument value.
#######################################
function get_cl_arg()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}
    local CL_TYPE_SUFFIX=${3}
    local CL_VALUE_PREFIX=${4:-""}

    echo "$ARG_NAME:$CL_TYPE_SUFFIX='$CL_VALUE_PREFIX$ARG_VALUE'"
}

#######################################
# Returns a formatted session argument (cl type=account hash).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_account_hash()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "account_hash" "account-hash-")
}

#######################################
# Returns a formatted session argument (cl type=account key).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_account_key()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "public_key")
}

#######################################
# Returns a formatted session argument (cl type=optional uref).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_opt_uref()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "opt_uref")
}

#######################################
# Returns a formatted session argument (cl type=string).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_string()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "string")
}

#######################################
# Returns a formatted session argument (cl type=u64).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_u64()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "U64")
}

#######################################
# Returns a formatted session argument (cl type=u256).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_u256()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "U256")
}

#######################################
# Returns a formatted session argument (cl type=u512).
# Arguments:
#   Argument name.
#   Argument value.
#######################################
function get_cl_arg_u512()
{
    local ARG_NAME=${1}
    local ARG_VALUE=${2}

    echo $(get_cl_arg $ARG_NAME $ARG_VALUE "U512")
}
