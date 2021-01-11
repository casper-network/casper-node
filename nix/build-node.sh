#!/bin/sh

set -eu

TAG=$(git describe --always --dirty)

nix-build --argstr tag "${TAG}" node-container.nix

echo "Created new docker image."
echo
echo "Load into local docker:"
echo "docker load -i ./result"
