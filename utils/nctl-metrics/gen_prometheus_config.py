#!/usr/bin/env python

import os

import toml

net_name = 'net-1'
nodes_dir = os.path.join(
        os.path.dirname(__file__),
        "..",
        "nctl",
        "assets",
        net_name,
        "nodes"
    )

# We start with the `mem_export` service.
addrs = ["localhost:8000"]

for node_dir in os.listdir(nodes_dir):
    node_path = os.path.join(nodes_dir, node_dir)
    if not os.path.isdir(node_path) or not node_dir.startswith("node-"):
        continue

    config = toml.load(open(os.path.join(node_path, 'config', 'node-config.toml')))
    addr = config["rest_server"]["address"].replace("0.0.0.0", "127.0.0.1")
    addrs.append(addr)


# Slightly dirty, we're not dealing with an extra dependency to generate YAML just yet and just
# abuse that pythons list display rendering is valid yaml.
cfg = """scrape_configs:
  - job_name: root_node
    scrape_interval: 5s
    static_configs:
      - targets: {}
""".format(addrs)

print(cfg)
