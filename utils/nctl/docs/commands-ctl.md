# NCTL Control Commands

## Overview

The aim of NCTL is to enable a user to spin up a test network within 15-20 seconds.  Once a network is up & running the user can control each node as well adding new nodes to the network.  Hereby are listed the set of NCTL commands to control a test network.

### nctl-clean node={X:-all}

Stops node X (if running) & deletes node logs & storage.

```
nctl-clean

nctl-clean node=all  (same as above)

nctl-clean node=3
```

### nctl-clean-logs node={X:-all}

Deletes node logs.

```
nctl-clean-logs

nctl-clean-logs node=all  (same as above)

nctl-clean-logs node=3
```

### nctl-interactive node={X:-1} loglevel={Y:-($RUST_LOG | debug)}

Starts (in interactive mode) node X with logging level set to Y.

```
nctl-interactive

nctl-interactive node=1  (same as above)

nctl-interactive node=3
```

### nctl-join node={X:-6} amount={Y:-1000000} rate={Z:-125}

Attempts to join node X to test network by submitting an auction bid of Y with a delegation rate of Z.  This command will await 3 eras after the auction bid has been submitted before starting node with a trusted hash. 

```
nctl-join

nctl-join node=6 amount=1000000 rate=125 (same as above)

nctl-join node=8 amount=500000 rate=250
```

### nctl-leave node={X:-6} amount={Y:-1000000}

Attempts to detach node X from test network by submitting an auction withdrawal Y.  This command awaits 1 era before stopping the node. 

```
nctl-leave

nctl-leave node=6 amount=1000000 (same as above)

nctl-leave node=8 amount=500000
```

### nctl-restart node={X:-all} clean={Y:-true}

Restarts node Y, if Y=all then all nodes are restarted.  Node storage state and logs are cleared by default.

```
nctl-restart

nctl-restart node=all clean=true (same as above)

nctl-restart node=3 clean=false
```

### nctl-rotate

Attempts to rotate a network's validator set - used to verify joining/leaving scenarios.

```
nctl-rotate
```

### nctl-start node={X:-all} hash=Y loglevel={Z:-($RUST_LOG | debug)}

Starts node X, if X=all then all nodes are started.  If trusted hash is specified then it is injected into node's config prior to spin-up.

```
nctl-start

nctl-start node=all loglevel=debug (same as above)

nctl-start node=3 hash=<A-TRUSTED_HASH> loglevel=info
```

### nctl-start-after-n-blocks node={W:-all} offset={X:-1} hash=Y loglevel={Z:-($RUST_LOG | debug)}

Starts node W after chain has advanced by X blocks.  If W=all then all nodes are started.  If hash is specified then it is injected into node's config prior to spin-up.

```
nctl-start-after-n-blocks

nctl-start-after-n-blocks node=all  (same as above)

nctl-start-after-n-blocks node=6 offset=4 hash=<A-TRUSTED_HASH>
```

### nctl-start-after-n-eras node={W:-all} offset={X:-1} hash=Y loglevel={Z:-($RUST_LOG | debug)}

Starts node W after chain has advanced by X eras.  If W=all then all nodes are started.  If hash is specified then it is injected into node's config prior to spin-up.

```
nctl-start-after-n-eras

nctl-start-after-n-eras node=all offset=1  (same as above)

nctl-start-after-n-eras node=8 offset=4 hash=<A-TRUSTED_HASH>
```

### nctl-status

Displays process status of all nodes.

```
nctl-status
```

### nctl-stop node={Y:-all}

Stops node Y, if Y=all then all nodes are stopped.

```
nctl-stop

nctl-stop node=all  (same as above)

nctl-stop node=3
```
