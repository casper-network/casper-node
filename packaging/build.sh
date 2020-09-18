#!/usr/bin/env bash

# Having to -f because of right from docker build. Need to fix.
rm -f ./share/*.deb
docker-compose up
docker-compose rm -y
docker rm packaging_deb_1804_1
docker rm packaging_deb_2004_1
