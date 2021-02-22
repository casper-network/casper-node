#!/usr/bin/env python

import os
import sys

import toml

net_name = "net-1"
nodes_dir = os.path.join(
    os.path.dirname(__file__), "..", "nctl", "assets", net_name, "nodes"
)

# We start with the `mem_export` service.
addrs = ["127.0.0.1:8000"]

for node_dir in os.listdir(nodes_dir):
    node_path = os.path.join(nodes_dir, node_dir)
    if not os.path.isdir(node_path) or not node_dir.startswith("node-"):
        continue

    cfg_path = os.path.join(node_path, "config", "1_0_0", "config.toml")
    try:
        config = toml.load(open(cfg_path))
        addr = config["rest_server"]["address"].replace("0.0.0.0", "127.0.0.1")
        addrs.append(addr)
    except Exception as e:
        sys.stderr.write("error loading {}\n".format(cfg_path))
        continue


# Slightly dirty, we're not dealing with an extra dependency to generate YAML just yet and just
# abuse that pythons list display rendering is valid yaml.
cfg = """scrape_configs:
  - job_name: nctl_scrape
    scrape_interval: 5s
    static_configs:
      - targets: {}
""".format(
    addrs
)

print(cfg)
