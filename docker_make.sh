
# Images used in this script are build in CasperLabs/buildenv repo

# This allows make commands without local build environment setup or
# using an OS version other than locally installed.

set -e

package="docker_make.sh"
valid_docker_images=("node-build-u1804" "node-build-u2004")
default_image=${valid_docker_images[0]}
function help {
  echo "$package - issue make commands in docker environment"
  echo " "
  echo "$package [options] command"
  echo " "
  echo "options:"
  echo "-h, --help             show brief help"
  echo "--image=DOCKER_IMAGE   specify docker image to use: ${valid_docker_images[*]}"
  echo "                       Image will default to ${default_image} if not given"
  echo
  echo "Ex: '$package all' will execute 'make all' on ${default_image}."
  echo
  echo "Note: The results will be in your ./target or ./target-as directory and"
  echo "      might not be compatible with your local system"
  exit 0
}

while test $# -gt 0; do
  case "$1" in
    -h|--help)
      help
      ;;
    --image*)
      image=`echo $1 | sed -e 's/^[^=]*=//g'`
      if [[ " ${valid_docker_images[*]} " == *" $image "* ]]; then
        docker_image=$image
      else
        echo "Invalid docker image passed in: $image"
        echo "Possible images are: ${valid_docker_images[*]}."
      fi
      shift
      ;;
    *)
      break
      ;;
  esac
done

if [ -z "$1" ]; then
  echo "make command not given."
#  echo "Using 'list' to show targets."
#  make_command="list"
  exit 1
else
  make_command="$1"
fi

if [ -z "$docker_image" ]; then
  echo "Defaulting build image to ${default_image}."
  docker_image=${default_image}
fi

docker pull casperlabs/${docker_image}:latest

# Getting user and group to chown/chgrp target folder from root at end.
# Cannot use the --user trick as cached .cargo in image is owned by root.
command="cd /casper-node; make ${make_command}; chown -R -f $(id -u):$(id -g) ./target ./target_as ./execution_engine_testing/casper_casper;"
docker run --rm --volume $(pwd):/casper-node casperlabs/${docker_image}:latest /bin/bash -c "${command}"