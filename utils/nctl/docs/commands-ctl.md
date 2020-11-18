# NCTL Control Commands

## Overview

The aim of NCTL is to enable a user to spin up a test network within 15-20 seconds.  Once a network is up & running the user should be able to control each of the node's within the network as well as add new nodes to the network.  Hereby are listed the set of NCTL commands to control a tst network

## Compiling network binaries

The NCTL library can be used to compile the node's binary set, i.e. node, client & smart contract binaries.  The NCTL library does not immediately copy the compiled binary set into a test directory, that is down when setting up test assets (see `nctl-assets-setup` below). 

### nctl-compile

Compiles casper node, client + client contracts using `make` + `cargo`.  


### nctl-compile-node

Compiles casper node using `make` + `cargo`.  


### nctl-compile-client

Compiles casper client + client contracts using `make` + `cargo`.  


## Managing network assets

### nctl-assets-ls

List previously created network assets.


### nctl-assets-setup net={W:-1} nodes={X:-5} users={Y:-5} bootstraps={Z:-1}

Sets up assets required to run a local N-node network - this includes binaries, chainspec, config, faucet, keys ... etc.

```
nctl-assets-setup

nctl-assets-setup net=1 nodes=5 users=5 bootstraps=1  (same as above)

nctl-assets-setup net=2 nodes=10 users=10 bootstraps=4
```

### nctl-assets-teardown net={X:-1}

Stops network & destroys all related assets.

```
nctl-assets-teardown

nctl-assets-teardown net=1  (same as above)

nctl-assets-teardown net=2
```

### nctl-assets-dump net={X:-1}

Dumps transient network assets such as logs + configuration.

```
nctl-assets-dump

nctl-assets-dump net=1  (same as above)
```

## Controlling network nodes

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

### nctl-start net={X:-1} node={Y:-all} loglevel={Z:-($RUST_LOG | debug)}

Starts node Y in network X.  If Y=all then all nodes in the network are restarted.  See note 3 above in repsec of logging level.

```
nctl-start

nctl-start net=1 node=all  (same as above)

nctl-start net=1 node=3
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
