# nctl Usage

Once activated, nctl commands can be used to setup & control nodes within local test network(s).  Whilst most nctl users will tend to focus upon testing a single network, developers may wish to test multiple networks in parallel so as to observe behavioural differences induced as a result of altering either the network's configuration or its binary set.  

This usage guide focusses upon the former use case, i.e. testing a single network, and thus all nctl commands described below are executed with their default values.  Please refer [here](commands.md) for full details of supported nctl commands.

## Step 0: Compile network binaries.

Prior to testing a network ensure that the binary sets are available:

```
nctl-compile
```

## Step 1: Create network assets.

- Once network binaries are available proceed to setup test network assets.  The following command instantiates the full set of assets required to run a 5 node local network with 5 users.  The assets are copied to `$NCTL/assets/net-1`, where $NCTL is the nctl home directory.

```
nctl-assets-setup
```

- Examining the contents of `$NCTL/assets/net-1` you will observe the following (self-explanatory) sub-folders:

```
/bin
/chainspec
/daemon
/faucet
/nodes
/users
```

- Examining the contents of `$NCTL/assets/net-1/nodes/node-1`, i.e. node 1, you will observe the following (self-explanatory) sub-folders:

```
/config
/keys
/logs
/storage
```

- Examining the contents of `$NCTL/assets/net-1/users/user-1`, i.e. user 1, you will find both cryptographic keys & account public key (hex) files. 

- Once assets have been created you are advised to review contents of toml files, i.e. `/chainspec/chainspec.toml` & the various `/nodes/node-X/config/node-config.toml` files.

- If you wish to test a modification to the node software, you can make the code modification, recompile the binary set, create a new set of network assets by incrementing the network identifier to 2.  At this point we will have 2 test networks ready to be run side by side.

- If you wish to test modifications to a network's chainspec, you can:

```
vi $NCTL/assets/net-1/chainspec/chainspec.toml
```

- If you wish to test modifications to a node's config, e.g. node 3, you can:

```
vi $NCTL/assets/net-1/nodes/node-3/config/node-config.toml
```

## Step 2: Start a node in interactive mode.

- Starting a node interactively is useful to verify that the network assets have been correctly established and that the network is ready for testing.  

```
nctl-interactive
```

## Step 3: Start a network in daemon mode.

- To start with all or a single node in daemonised mode (this is the preferred modus operandi):

```
# Start all nodes in daemon mode.
nctl-start

# Start node 1 in daemon mode.
nctl-start node=1
```

- To view process status of all daemonised nodes:

```
nctl-status
```

- To restart either a single or all daemonised nodes:

```
# Restart all nodes.
nctl-restart 

# Restart node 1.
nctl-restart node=1
```

- To stop either a single or all daemonised nodes:

```
# Stop all nodes.
nctl-stop 

# Stop node 1.
nctl-stop node=1
```

## Step 4: Dump logs & other files.

Upon observation of a network behavioural anomaly you can dump relevant assets such as logs & configuration as follows:

```
# Writes dumped files -> $NCTL/dumps/net-1
nctl-assets-dump
```

## Step 5: Viewing information.

You can view chain, faucet, node & user information using the set of `nctl-view-*` commands.  See [here](commands.md) for further information.

## Step 6: End testing session.

To teardown a network once a testing session is complete:

```
# Delete previously created assets and stops all running nodes.
nctl-assets-teardown
```

## Summary

Using nctl one can spin up either a single or multiple test networks.  Each network is isolated in terms of its assets - this includes port numbers.  The nctl commands parameter defaults are set for the general use case of testing a single local 5 node network.  You are encouraged to integrate nctl into your daily workflow so as to standardise the manner in which the network is tested in a localised setting.
