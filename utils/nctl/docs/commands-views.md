# NCTL View Commands

## Overview

NCTL provides well over 30 commands for viewing information related to the network being tested.  Such information can be categorised as pertaining to either the chain itself, the network's faucet account, the set of network validators or nodes, and the set of test user accounts.  NCTL thus provides some essential insights into the data types flowing through the fabric of the chain.

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

### nctl-view-chain-auction-info net={X:-1} node={Y:-1}

Displays details at node Y on network Z of the Proof of Stake auction contract.

```
nctl-view-chain-auction-info

nctl-view-chain-auction-info net=1 node=1 (same as above)

nctl-view-chain-auction-info net=1 node=3  
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

### nctl-view-chain-spec net={X:-1} 

Displays chainspec file pertaining to network X.

```
nctl-view-chain-spec 

nctl-view-chain-spec net=1 (same as above)

nctl-view-chain-spec net=3 (same as above)
```

### nctl-view-chain-spec-accounts net={X:-1} 

Displays chainspec accounts.csv file pertaining to network X.

```
nctl-view-chain-spec-accounts 

nctl-view-chain-spec-accounts net=1 (same as above)

nctl-view-chain-spec-accounts net=3 (same as above)
```

### nctl-view-chain-state-root-hash net={X:-1} node={Y:-1}

Displays a chain's state root hash.

```
nctl-view-chain-state-root-hash 

nctl-view-chain-state-root-hash net=1 node=1 (same as above)
```

## Viewing faucet information

### nctl-view-faucet-account net={X:-1}

Displays on-chain faucet account information.

```
nctl-view-faucet-account

nctl-view-faucet-account net=1  (same as above)
```

### nctl-view-faucet-account-balance net={X:-1}

Displays main purse balance of faucet account associated with network X.

```
nctl-view-faucet-account-balance

nctl-view-faucet-account-balance net=1  (same as above)
```

### nctl-view-faucet-account-hash net={X:-1}

Displays account hash of the faucet associated with network X.

```
nctl-view-faucet-account-hash

nctl-view-faucet-account-hash net=1  (same as above)
```

### nctl-view-faucet-account-key net={X:-1}

Displays public key in HEX format of the faucet associated with network X.

```
nctl-view-faucet-account-key

nctl-view-faucet-account-key net=1  (same as above)
```

### nctl-view-faucet-secret-key-path net={X:-1} user={Y:-all}

Displays path to secret key in PEM format of the faucet associated with network X.

```
nctl-view-faucet-secret-key-path

nctl-view-faucet-secret-key-path net=1   (same as above)

nctl-view-faucet-secret-key-path net=2 
```

## Viewing node information

### nctl-view-node-config net={X:-1} node={Y:-1}

Displays configuraiton file node Y in network X.

```
nctl-view-node-config

nctl-view-node-config net=1 node=1  (same as above)

nctl-view-node-config net=1 node=3
```

### nctl-view-node-log net={X:-1} node={Y:-1} typeof={Z:-stdout}

Displays log of node Y in network X.  Z=stdout|stderr.

```
nctl-view-node-log

nctl-view-node-log net=1 node=1 typeof=stdout  (same as above)

nctl-view-node-log net=1 node=3 typeof=stderr
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

### nctl-view-user-account net={X:-1} user={Y:-1}

Displays on-chain user account information.

```
nctl-view-user-account

nctl-view-user-account net=1 user=1  (same as above)
```

### nctl-view-user-account-balance net={X:-1} node={Y:-1} user={Z:-1}

Displays main purse balance of user Y.

```
nctl-view-user-account-balance

nctl-view-user-account-balance net=1 node=1 user=1  (same as above)
```

### nctl-view-user-account-key net={X:-1} user={Y:-all}

Displays public key in HEX format of user Y.

```
nctl-view-user-account-key

nctl-view-user-account-key net=1 user=all  (same as above)

nctl-view-user-account-key net=1 user=1  
```

### nctl-view-user-account-hash net={X:-1} user={Y:-all}

Displays account hash of user Y.

```
nctl-view-user-account-hash

nctl-view-user-account-hash net=1 user=all  (same as above)

nctl-view-user-account-hash net=1 user=1  
```

### nctl-view-user-secret-key-path net={X:-1} user={Y:-all}

Displays path to secret key in PEM format of user Y.

```
nctl-view-user-secret-key-path

nctl-view-user-secret-key-path net=1 user=all  (same as above)

nctl-view-user-secret-key-path net=1 user=1
```

## Viewing validator information

### nctl-view-validator-account net={X:-1} user={Y:-1}

Displays on-chain validator account information.

```
nctl-view-validator-account

nctl-view-validator-account net=1 node=1  (same as above)
```

### nctl-view-validator-account-balance net={X:-1} node={Y:-all}

Displays main purse balance of validator Y.

```
nctl-view-validator-account-balance

nctl-view-validator-account-balance net=1 node=all  (same as above)

nctl-view-validator-account-balance net=1 node=1
```

### nctl-view-validator-account-key net={X:-1} node={Y:-all}

Displays public key in HEX format of validator Y.

```
nctl-view-validator-account-key

nctl-view-validator-account-key net=1 node=all  (same as above)

nctl-view-validator-account-key net=1 node=1  
```

### nctl-view-validator-account-hash net={X:-1} node={Y:-all}

Displays account hash of validator Y.

```
nctl-view-validator-account-hash

nctl-view-validator-account-hash net=1 node=all  (same as above)

nctl-view-validator-account-hash net=1 node=1  
```

### nctl-view-validator-secret-key-path net={X:-1} node={Y:-all}

Displays path to secret key in PEM format of validator Y.

```
nctl-view-validator-secret-key-path

nctl-view-validator-secret-key-path net=1 node=all  (same as above)

nctl-view-validator-secret-key-path net=1 node=1
```
