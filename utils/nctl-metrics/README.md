# Metrics for nctl

A small setup that runs enough containers to get metrics working when using nctl. Contains

* a python script that will scrape memory metrics from the OS and make them available via HTTP for prometheus,
* a generator for a prometheus configuration file based on current nctl assets (only `net-1` supported), and
* a supervisord configuration to run the generator and prometheus conveniently.

## How to run

1. Ensure nctl has generated assets (`nctl-assets-setup`).
2. Run `supervisord -c utils/nctl-metrics/supervisord.conf`.
3. Navigate to localhost:9090 and watch metrics.

## Common Issues

* Why am I not getting any memory metrics?

Check the logs `memory-stats-collector.log`. If there are messages stating `AF_UNIX path too long`, the root path of your `casper-node/utils/nctl/assets/...`, which contains the `supervisord.sock` directory is too long.
