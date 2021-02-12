# Metrics for nctl

A small setup that runs enough containers to get metrics working when using nctl.

## How to run

1. Ensure nctl has generated assets (`nctl-assets-setup`).
2. Run `supervisord -c utils/nctl-metrics/supervisord.conf`.
3. Navigate to <http://localhost:9090> and watch metrics.

## Architecture

The directory contains

* a python script that will scrape memory metrics from the OS and make them available via HTTP for prometheus,
* a generator for a prometheus configuration file based on current nctl assets (only `net-1` supported), and
* a supervisord configuration to run the generator and prometheus conveniently.

## Metrics offered

In addition to the usual node metrics, the following metrics are available:

* `os_mem_rss_bytes`
* `os_mem_vms_bytes`
* `os_mem_shared_bytes`
* `os_mem_text_bytes`
* `os_mem_lib_bytes`
* `os_mem_data_bytes`
* `os_mem_dirty_bytes`

Each has a `node` label indicating which node's memory usage is shown.

## Common Issues

* Why am I not getting any memory metrics?

Check the logs `memory-stats-collector.log`. If there are messages stating `AF_UNIX path too long`, the root path of your `casper-node/utils/nctl/assets/...`, which contains the `supervisord.sock` directory is too long.
