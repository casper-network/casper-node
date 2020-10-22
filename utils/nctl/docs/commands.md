# nctl Commands

## Overview

Upon successful setup, nctl commands are made available via aliases for execution from within a terminal session.  All such commands are prefixed by `nctl-` and allow you to perform various tasks as detailed below.

NOTE 1: all network & node ordinal identifiers are 1 based.

NOTE 2: all command parameterrs have default values to simplify the general case of testing a single local network.

NOTE 3: when executing either the `nctl-interactive` or `nctl-start` commands, the node logging level output can be assigned by passing in the `loglevel` parameter.  If you do not pass in this variable then current it defaults either to the current value of RUST_LOG or `debug`.


## Compiling network binaries

The nctl library can be used to compile the node's binary set, i.e. node, client & smart contract binaries.  The nctl library does not immediately copy the compiled binary set into a test directory, that is down when setting up test assets (see `nctl-assets-setup` below). 

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

### nctl-interactive net={X:-1} node={Y:-1} loglevel={Z:-($NCTL_NODE_LOG_LEVEL | debug)}

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

### nctl-start net={X:-1} node={Y:-all} loglevel={Z:-($NCTL_NODE_LOG_LEVEL | debug)}

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

## Viewing chain information

### nctl-view-chain-account net={W:-1} node={X:-1} account-key=Y root-hash=Z

Displays result of performing a state query by account key Y at state root hash Z.

```
nctl-view-chain-account account-key=AN_ACCOUNT_KEY root-hash=A_STATE_ROOT_HASH

nctl-view-chain-account net=1 node=1 account-key=AN_ACCOUNT_KEY root-hash=A_STATE_ROOT_HASH (same as above)
```

### nctl-view-chain-account-balance net={W:-1} node={X:-1} purse-uref=Y root-hash=Z

Displays balance of an account purse with uref Y at state root hash Z.

```
nctl-view-chain-account-balance purse-uref=A_PURSE_UREF root-hash=A_STATE_ROOT_HASH

nctl-view-chain-account-balance net=1 node=1 purse-uref=A_PURSE_UREF root-hash=A_STATE_ROOT_HASH (same as above)
```

### nctl-view-chain-block net={X:-1} node={Y:-1} block=Z

Displays details of block Z at node Y on network Z.

```
nctl-view-chain-block block=A_BLOCK_HASH

nctl-view-chain-block net=1 node=1 block=A_BLOCK_HASH (same as above)
```

### nctl-view-chain-deploy net={X:-1} node={Y:-1} deploy=Z

Displays details of deploy Z at node Y on network Z.

```
nctl-view-chain-deploy deploy=A_DEPLOY_HASH

nctl-view-chain-deploy net=1 node=1 deploy=A_DEPLOY_HASH (same as above)
```

## Viewing faucet information

### nctl-view-faucet-account-key net={X:-1}

Displays public key in HEX format of the faucet associated with network X.

```
nctl-view-faucet-account-key

nctl-view-faucet-account-key net=1  (same as above)
```

## Viewing node information

### nctl-view-node-log net={X:-1} node={Y:-1} typeof={Z:-stdout}

Displays log of node Y in network X.  Z=stdout|stderr.

```
nctl-view-node-log

nctl-view-node-log net=1 node=1 typeof=stdout  (same as above)

nctl-view-node-log net=1 node=3 typeof=stderr
```

### nctl-view-node-metrics net={X:-1} node={Y:-all} metric={Z:-all}

Renders metrics of node Y in network X to stdout.  Assign the metric parameter to filter accordingly.

```
nctl-view-node-metrics

nctl-view-node-metrics net=1 node=all metric=all (same as above)

nctl-view-node-metrics net=1 node=all metric=scheduler_queue_regular_count

nctl-view-node-metrics net=1 node=2 metric=runner_events
```

### nctl-view-node-metric-finalised-block-count net={X:-1} node={Y:-all}

Renders count of finalised blocks at node Y in network X to stdout.

```
nctl-view-node-metric-finalised-block-count

nctl-view-node-metric-finalised-block-count net=1 node=all (same as above)
```

### nctl-view-node-metric-pending-deploy net={X:-1} node={Y:-all}

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

## Viewing user information

### nctl-view-user-account-key net={X:-1} user=Y

Displays public key in HEX format of user Y.

```
nctl-view-user-account-key

nctl-view-user-account-key net=1 user=1  (same as above)
```

### nctl-view-user-secret-key-path net={X:-1} user=Y

Displays path to secret key in PEM format of user Y.

```
nctl-view-user-secret-key-path

nctl-view-user-secret-key-path net=1 user=1  (same as above)
```

## Dispatching deploys

### nctl-wg-100 net={X:-1} node={Y:-1} payment={P:-200000} gas={G:-10} transfers={T:-100} interval={I:-0.01} user={U:-1}

Dispatches to node Y in network X, T wasmless transfers from network faucet to user #U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-wg-100

nctl-wg-100 net=1 node=1 payment=200000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-wg-100 transfers=10000 interval=0.001
```

NOTE - this command has a synonym: `nctl-do-transfer`

### nctl-wg-110 net={X:-1} node={Y:-1} payment={P:-200000} gas={G:-10} transfers={T:-100} interval={I:-0.01} user={U:-1}

Dispatches to node Y in network X, T wasm based transfers from network faucet to user #U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-wg-110

nctl-wg-110 net=1 node=1 payment=200000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-wg-110 transfers=10000 interval=0.001
```

NOTE - this command has a synonym: `nctl-do-transfer-wasm`

### nctl-wg-200 net={X:-1} node={Y:-1} amount={A:-1000000} rate={R:-125} payment={P:-200000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from user #U, a Proof-Of-Stake auction bid **submission** for amount A (motes) with a delegation rate of R.  Displays relevant deploy hash for subsequent querying.

```
nctl-wg-200

nctl-wg-200 net=1 node=1 amount=1000000 rate=125 payment=200000 gas=10 user=1  (same as above)

nctl-wg-200 amount=2000000 rate=250
```

NOTE - this command has a synonym: `nctl-do-auction-submit`

### nctl-wg-201 net={X:-1} node={Y:-1} amount={A:-1000000} payment={P:-200000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from user #U, a Proof-Of-Stake auction bid **withdrawal** for amount A (motes).  Displays relevant deploy hash for subsequent querying.

```
nctl-wg-201

nctl-wg-201 net=1 node=1 amount=1000000 payment=200000 gas=10 user=1  (same as above)

nctl-wg-201 amount=2000000
```

NOTE - this command has a synonym: `nctl-do-auction-withdraw`

### nctl-wg-210 net={X:-1} node={Y:-1} amount={A:-1000000} payment={P:-200000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from user #U, a Proof-Of-Stake **delegate** bid for amount A (motes).  Displays relevant deploy hash for subsequent querying.

```
nctl-wg-210

nctl-wg-210 net=1 node=1 amount=1000000 payment=200000 gas=10 user=1  (same as above)

nctl-wg-210 amount=2000000
```

NOTE - this command has a synonym: `nctl-do-auction-delegate`

### nctl-wg-211 net={X:-1} node={Y:-1} amount={A:-1000000} payment={P:-200000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from user #U, a Proof-Of-Stake **undelegate** bid for amount A (motes).  Displays relevant deploy hash for subsequent querying.

```
nctl-wg-211

nctl-wg-211 net=1 node=1 amount=1000000 payment=200000 gas=10 user=1  (same as above)

nctl-wg-211 amount=2000000
```

NOTE - this command has a synonym: `nctl-do-auction-undelegate`
