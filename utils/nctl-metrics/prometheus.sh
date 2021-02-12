#!/bin/sh

#: Run a prometheus instance that collects metrics from a local nctl network.

cd $(dirname $0))

docker run --net=host -p 9090:9090 -v /tmp/prometheus.yml:/etc/prometheus/prometheus.yml prom/prometheus
