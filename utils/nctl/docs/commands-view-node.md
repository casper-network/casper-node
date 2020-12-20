# NCTL Viewing Node Information

### nctl-view-node-config net={X:-1} node={Y:-1}

Displays configuraiton file node Y in network X.

```
nctl-view-node-config

nctl-view-node-config net=1 node=1  (same as above)

nctl-view-node-config net=1 node=3
```

### nctl-view-node-error-log net={X:-1} node={Y:-1}

Displays error log of node Y in network X.

```
nctl-view-node-error-log

nctl-view-node-error-log net=1 node=1  (same as above)

nctl-view-node-error-log net=1 node=3 
```

### nctl-view-node-log net={X:-1} node={Y:-1}

Displays stdout log of node Y in network X.

```
nctl-view-node-log

nctl-view-node-log net=1 node=1 (same as above)

nctl-view-node-log net=1 node=3 
```

### nctl-view-node-finalised-block-count net={X:-1} node={Y:-all}

Renders count of finalised blocks at node Y in network X to stdout.

```
nctl-view-node-metric-finalised-block-count

nctl-view-node-metric-finalised-block-count net=1 node=all (same as above)
```

### nctl-view-node-metrics net={X:-1} node={Y:-all} metric={Z:-all}

Renders metrics of node Y in network X to stdout.  Assign the metric parameter to filter accordingly.

```
nctl-view-node-metrics

nctl-view-node-metrics net=1 node=all metric=all (same as above)

nctl-view-node-metrics net=1 node=all metric=scheduler_queue_regular_count

nctl-view-node-metrics net=1 node=2 metric=runner_events
```

### nctl-view-node-pending-deploy-count net={X:-1} node={Y:-all}

Renders count of pending deploys at node Y in network X to stdout.

```
nctl-view-node-metric-pending-deploy

nctl-view-node-metric-pending-deploy net=1 node=all (same as above)
```

### nctl-view-node-peers net={X:-1} node={Y:-all}

Renders peers of node Y in network X to stdout.

```
nctl-view-node-peers

nctl-view-node-peers net=1 node=all  (same as above)

nctl-view-node-peers net=1 node=3
```

### nctl-view-node-ports net={X:-1} node={Y:-all}

Renders ports of node Y in network X to stdout.

```
nctl-view-node-ports

nctl-view-node-ports net=1 node=all  (same as above)

nctl-view-node-ports net=1 node=3
```

### nctl-view-node-rpc-schema net={X:-1} node={Y:-1}

Renders RPC schema at node Y in network X to stdout.

```
nctl-view-node-rpc-schema

nctl-view-node-rpc-schema net=1 node=1  (same as above)

nctl-view-node-rpc-schema net=1 node=3
```

### nctl-view-node-status net={X:-1} node={Y:-all}

Renders status of node Y in network X to stdout.

```
nctl-view-node-status

nctl-view-node-status net=1 node=all  (same as above)

nctl-view-node-status net=1 node=3
```

### nctl-view-node-storage net={X:-1} node={Y:-all}

Renders storage stats of node Y in network X to stdout.

```
nctl-view-node-storage

nctl-view-node-storage net=1 node=all  (same as above)

nctl-view-node-storage net=1 node=3
```
