#!/bin/sh

#: Run a prometheus instance that collects metrics from a local nctl network.

cd $(dirname $0)

PROMETHEUS_TAG=prom/prometheus

echo "Genarating config."
./gen_prometheus_config.py > prometheus.yml

echo "Starting prometheus."
exec docker run \
  -i \
  --rm \
  --net=host \
  -p 9090:9090 \
  -v $(pwd)/prometheus.yml:/etc/prometheus/prometheus.yml \
  ${PROMETHEUS_TAG}
