# NCTL Deploy Dispatch Commands

## Overview

NCTL provides commands for dispatching deploys (i.e. transactions) to a target test network.  The deploys may pertain to either simple token transfers, ERC-20 token smart contracts, and/or the network's Proof of Stake contract(s).

Many of the below commands take the following arguments as default:

```
net={A:-1} node={B:-1} gas={C:-1000000000} payment={D:-10}
```

## Simple Token Transfers 

### nctl-transfer-native net={X:-1} node={Y:-1} payment={P:-1000000000} gas={G:-10} transfers={T:-100} interval={I:-0.01} user={U:-1}

Dispatches to node Y in network X, T wasmless transfers from network faucet to user U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-transfer-native

nctl-transfer-native net=1 node=1 payment=1000000000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-transfer-native transfers=10000 interval=0.001
```

Note: has a synonym: `nctl-transfer`

### nctl-do-transfer-wasm net={X:-1} node={Y:-1} payment={P:-1000000000} gas={G:-10} transfers={T:-100} interval={I:-0.01} user={U:-1}

Dispatches to node Y in network X, T wasm based transfers from network faucet to user U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-do-transfer-wasm

nctl-do-transfer-wasm net=1 node=1 payment=1000000000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-do-transfer-wasm transfers=10000 interval=0.001
```

## Proof-Of-Stake 

### nctl-auction-bid bidder={B:-6} amount={A:-1000000} rate={R:-125} net={X:-1} node={Y:-1} payment={P:-1000000000} gas={G:-10}

Dispatches to node Y in network X from bidder B, a Proof-Of-Stake auction bid **submission** for amount A (motes) with a delegation rate of R.  Displays relevant deploy hash for subsequent querying.

```
nctl-auction-bid

nctl-auction-bid bidder=6 amount=6000000000000000 rate=125 net=1 node=1 payment=1000000000 gas=10  (same as above)

nctl-auction-bid bidder=7 amount=7000000000000000 rate=250 net=1 node=4 payment=5000000000 gas=20
```

### nctl-auction-withdraw bidder={B:-6} amount={A:-1000000} rate={R:-125} net={X:-1} node={Y:-1} payment={P:-1000000000} gas={G:-10}

Dispatches to node Y in network X from user U, a Proof-Of-Stake auction bid **withdrawal** for amount A (motes).  Displays relevant deploy hash for subsequent querying.

```
nctl-auction-withdraw

nctl-auction-withdraw bidder=6 amount=6000000000000000 net=1 node=1 payment=1000000000 gas=10  (same as above)

nctl-auction-withdraw bidder=7 amount=7000000000000000 net=1 node=4 payment=5000000000 gas=20
```

### nctl-auction-delegate amount={A:-1000000} delegator={D:-1} validator={V:-1} net={X:-1} node={Y:-1} payment={P:-1000000000} gas={G:-10}

Dispatches to node Y in network X from delegator D, a Proof-Of-Stake **delegate** bid for amount A (motes) nominating validator V.  Displays relevant deploy hash for subsequent querying.

```
nctl-auction-delegate

nctl-auction-delegate amount=1000000 delegator=1 validator=1 net=1 node=1 payment=1000000000 gas=10 user=1  (same as above)

nctl-auction-delegate amount=2000000 delegator=3 validator=4
```

### nctl-auction-undelegate net={X:-1} node={Y:-1} amount={A:-1000000} payment={P:-1000000000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from delegator D, a Proof-Of-Stake **undelegate** bid for amount A (motes) unnominating validator V.  Displays relevant deploy hash for subsequent querying.

```
nctl-auction-undelegate

nctl-auction-undelegate amount=1000000 delegator=1 validator=1 net=1 node=1 payment=1000000000 gas=10 user=1  (same as above)

nctl-auction-undelegate amount=2000000 delegator=3 validator=4
```

## ERC-20 Token Standard 

### nctl-erc20-approve owner={X:-1} spender={Y:-1} amount={Z:-1000000000} 

Allows a user (the spender) to withdraw upto Z tokens from the account of a token holder (the owner).  

```
nctl-erc20-approve 

nctl-erc20-approve owner=1 spender=2 amount=1000000000  (same as above)

nctl-erc20-approve owner=2 spender=4 amount=4400000000  (same as above)
```

### nctl-erc20-fund-users amount={X:-1000000000} 

Transfers from the installed ERC-20 smart contract X tokens to each of the test user accounts.  

```
nctl-erc20-fund-users 

nctl-erc20-fund-users amount=1000000000  (same as above)

nctl-erc20-fund-users amount=4440000000  
```

### nctl-erc20-install name={X:-Acme Token} symbol={Y:-Acme} supply={Z:-1e33}  

Installs the ERC0-20 smart contract under the network's faucet account.  

NOTE : The compiled wasm file must have been previously moved to the test network's bin folder (e.g. `casper-node/utils/nctl/assets/net-1/bin`).  For further information refer to the network's smart contract SDK.

```
nctl-erc20-install 

nctl-erc20-install name="Acme Token" symbol="ACME" supply=1000000000000000000000000000000000  (same as above)

nctl-erc20-install name="Casper Labs" symbol="CSPR" supply=1000000000000000000000000000
```

### nctl-erc20-transfer from={X:-1} to={Y:-1} amount={Z:-1000000000} 

Transfers Z tokens from user X to user Y.  

```
nctl-erc20-transfer-from 

nctl-erc20-transfer-from from=1 to=2 amount=1000000000  (same as above)

nctl-erc20-transfer-from from=2 to=5 amount=4440000000  
```

### nctl-erc20-view-balances net={X:-1} node={Y:-1} 

Renders ERC-20 token balances of the network's faucet and user accounts.  

```
nctl-erc20-view-balances 

nctl-erc20-view-balances net=1 node=1  (same as above)

nctl-erc20-view-balances net=2 node=4
```

### nctl-erc20-view-details net={X:-1} node={Y:-1} 

Renders information related to the installed smart contract, e.g. contract hash.  

```
nctl-erc20-view-details 

nctl-erc20-view-details net=1 node=1  (same as above)

nctl-erc20-view-details net=2 node=4
```
