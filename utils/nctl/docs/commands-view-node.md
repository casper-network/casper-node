# NCTL Viewing Node Information

### nctl-view-node-config node={X:-1}

Displays configuraiton file node X.

```
nctl-view-node-config

nctl-view-node-config node=1  (same as above)

nctl-view-node-config node=3
```

### nctl-view-node-error-log node={X:-1}

Displays error log of node X.

```
nctl-view-node-error-log

nctl-view-node-error-log node=1  (same as above)

nctl-view-node-error-log node=3 
```

### nctl-view-node-log node={X:-1}

Displays stdout log of node X.

```
nctl-view-node-log

nctl-view-node-log node=1 (same as above)

nctl-view-node-log node=3 
```

### nctl-view-node-finalised-block-count node={X:-all}

Renders count of finalised blocks at node X to stdout.

```
nctl-view-node-metric-finalised-block-count

nctl-view-node-metric-finalised-block-count node=all (same as above)
```

### nctl-view-node-metrics node={X:-all} metric={Y:-all}

Renders metrics of node X to stdout.  Assign the metric parameter to filter accordingly.

```
nctl-view-node-metrics

nctl-view-node-metrics node=all metric=all (same as above)

nctl-view-node-metrics node=all metric=scheduler_queue_regular_count

nctl-view-node-metrics node=2 metric=runner_events
```

### nctl-view-node-pending-deploy-count node={X:-all}

Renders count of pending deploys at node X to stdout.

```
nctl-view-node-metric-pending-deploy

nctl-view-node-metric-pending-deploy node=all (same as above)
```

### nctl-view-node-peers node={X:-all}

Renders peers of node X to stdout.

```
nctl-view-node-peers

nctl-view-node-peers node=all  (same as above)

nctl-view-node-peers node=3
```

### nctl-view-node-ports node={X:-all}

Renders ports of node X to stdout.

```
nctl-view-node-ports

nctl-view-node-ports node=all  (same as above)

nctl-view-node-ports node=3
```

### nctl-view-node-rpc-schema node={X:-1}

Renders RPC schema at node X to stdout.

```
nctl-view-node-rpc-schema

nctl-view-node-rpc-schema node=1  (same as above)

nctl-view-node-rpc-schema node=3
```

### nctl-view-node-status node={X:-all}

Renders status of node X to stdout.

```
nctl-view-node-status

nctl-view-node-status node=all  (same as above)

nctl-view-node-status node=3
```

### nctl-view-node-storage node={X:-all}

Renders storage stats of node X to stdout.

```
nctl-view-node-storage

nctl-view-node-storage node=all  (same as above)

nctl-view-node-storage node=3
```
