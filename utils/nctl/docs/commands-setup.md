# NCTL Setup Commands

## Overview

The aim of NCTL is to enable a user to spin up a test network within 15-20 seconds.  Once a network is up & running the user should be able to control each of the node's within the network as well as add new nodes to the network.  Hereby are listed the set of NCTL commands to setup assets (binaries, config files, directories ... etc ) associated with a test network.

## Compiling network binaries

The NCTL library can be used to compile the node's binary set, i.e. node, client & smart contract binaries.  Note that NCTL library does not immediately copy compiled binary sets into a test directory, that is done whilst setting up test assets (see `nctl-assets-setup` below). 

### nctl-compile

Compiles casper node, node launcher, client + client contracts using `make` + `cargo`. Note: this command has an optional -d flag that can be use to compile in debug mode. Both the environment variable, as well as the new command line flag, will allow for debug compilation.

### nctl-compile-node

Compiles casper node using `make` + `cargo`.  

### nctl-compile-node-launcher

Compiles casper node launcher using `cargo`.  

### nctl-compile-client

Compiles casper client + client contracts using `make` + `cargo`.  

## Managing network assets

### nctl-assets-ls

List previously created network assets.

### nctl-assets-setup net={V:-1} nodes={W:-5} delay={X:-30} accounts_path={Y:-""} chainspec_path={Z:-"casper-node/resources/local/chainspec.toml.in"}

Sets up assets required to run a local network - this includes binaries, chainspec, config, faucet, keys ... etc.  NCTL creates assets for 2 nodesets: genesis & non-genesis - this permits testing nodeset rotation scenarios (see `nctl-rotate`).  Thus if nodes=5, then assets for 10 nodes are generated in total.  

If `accounts_path` points to a valid custom accounts.toml template file, then the template is copied, & parsed.  The parsing process injects faucet, validator and user public keys into the copied template file.  An example custom accounts.toml can be inspected [here](example-custom-accounts.toml). 

If `chainspec_path` points to a valid custom chainspec.toml, then the template is copied across to the test network asset set.

```
nctl-assets-setup

nctl-assets-setup net=1 nodes=5 delay=30  (same as above)

nctl-assets-setup net=2 nodes=10 delay=60
```

### nctl-assets-teardown net={X:-1}

Stops network & destroys all related assets.

```
nctl-assets-teardown

nctl-assets-teardown net=1  (same as above)

nctl-assets-teardown net=2
```

### nctl-assets-dump 

Dumps transient network assets such as logs + configuration.

```
nctl-assets-dump
```
