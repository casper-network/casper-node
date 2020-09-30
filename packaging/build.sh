#!/usr/bin/env bash

# Having to -f because of right from docker build. Need to fix.
rm -f ./share/*.deb
docker-compose up || true
docker-compose rm -vfs

