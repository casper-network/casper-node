# NCTL Control Commands

## Overview

The aim of NCTL is to enable a user to spin up a test network within 15-20 seconds.  Once a network is up & running the user can control each node as well adding new nodes to the network.  Hereby are listed the set of NCTL commands to control a test network.

### nctl-interactive net={X:-1} node={Y:-1} loglevel={Z:-($RUST_LOG | debug)}

Starts (in interactive mode) node Y in network X.  See note 3 above in repsec of logging level.

```
nctl-interactive

nctl-interactive net=1 node=1  (same as above)

nctl-interactive net=1 node=3
```

### nctl-log-reset net={X:-1} node={Y:-all}

Resets logs of node Y in network X.  If Y=all then the logs of all nodes are reset.

```
nctl-log-reset

nctl-log-reset net=1 node=all  (same as above)

nctl-log-reset net=1 node=3
```

### nctl-restart net={X:-1} node={Y:-all}

Restarts node Y in network X.  If Y=all then all nodes in the network are restarted.

```
nctl-restart

nctl-restart net=1 node=all  (same as above)

nctl-restart net=1 node=3
```

### nctl-rotate net={X:-1}

Attempts to rotate a network's validator set - used to verify joining/leaving scenarios.

```
nctl-rotate

nctl-rotate net=1  (same as above)
```

### nctl-start net={X:-1} node={Y:-all} loglevel={Z:-($RUST_LOG | debug)}

Starts node Y in network X.  If Y=all then all nodes in the network are started.  

```
nctl-start

nctl-start net=1 node=all  (same as above)

nctl-start net=1 node=3
```

### nctl-start-after-n-blocks net={W:-1} node={X:-all} offset={Y:-1} loglevel={Z:-($RUST_LOG | debug)}

Starts node Y in network X after chain has advanced by Z blocks.  If Y=all then all nodes in the network are started.  

```
nctl-start-after-n-blocks

nctl-start-after-n-blocks net=1 node=all  (same as above)

nctl-start-after-n-blocks net=1 node=6 offset=4
```

### nctl-start-after-n-eras net={W:-1} node={X:-all} offset={Z:-1} loglevel={Y:-($RUST_LOG | debug)}

Starts node Y in network X after chain has advanced by Z eras.  If Y=all then all nodes in the network are started.  

```
nctl-start-after-n-eras

nctl-start-after-n-eras net=1 node=6 offset=1  (same as above)

nctl-start-after-n-eras net=1 node=8 offset=4
```

### nctl-status net={X:-1}

Displays status of all nodes in network X.

```
nctl-status

nctl-status net=1  (same as above)
```

### nctl-stop net={X:-1} node={Y:-all}

Stops node Y in network X.  If Y=all then all nodes in the network are stopped.

```
nctl-stop

nctl-stop net=1 node=all  (same as above)

nctl-stop net=1 node=3
```
