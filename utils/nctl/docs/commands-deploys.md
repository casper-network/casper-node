# NCTL Deploy Dispatch Commands

## Overview

NCTL provides many commands for dispatching deploys (i.e. transactions) to the target test network.  The deploys may pertain to either simple token transfers, ERC-20 token smart contracts, and/or the network's Proof of Stake contract(s).

Many of the below commands take the following arguments as default:

```
net={A:-1} node={B:-1} gas={C:-1000000000} payment={D:-10}
```

## Simple Token Transfers 

### nctl-do-transfer net={X:-1} node={Y:-1} payment={P:-200000} gas={G:-10} transfers={T:-100} interval={I:-0.01} user={U:-1}

Dispatches to node Y in network X, T wasmless transfers from network faucet to user U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-do-transfer

nctl-do-transfer net=1 node=1 payment=200000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-do-transfer transfers=10000 interval=0.001
```

### nctl-do-transfer-wasm net={X:-1} node={Y:-1} payment={P:-200000} gas={G:-10} transfers={T:-100} interval={I:-0.01} user={U:-1}

Dispatches to node Y in network X, T wasm based transfers from network faucet to user U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-do-transfer-wasm

nctl-do-transfer-wasm net=1 node=1 payment=200000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-do-transfer-wasm transfers=10000 interval=0.001
```

## Proof-Of-Stake 

### nctl-do-auction-submit net={X:-1} node={Y:-1} amount={A:-1000000} rate={R:-125} payment={P:-200000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from user U, a Proof-Of-Stake auction bid **submission** for amount A (motes) with a delegation rate of R.  Displays relevant deploy hash for subsequent querying.

```
nctl-do-auction-submit

nctl-do-auction-submit net=1 node=1 amount=1000000 rate=125 payment=200000 gas=10 user=1  (same as above)

nctl-do-auction-submit amount=2000000 rate=250
```

### nctl-do-auction-withdraw net={X:-1} node={Y:-1} amount={A:-1000000} payment={P:-200000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from user U, a Proof-Of-Stake auction bid **withdrawal** for amount A (motes).  Displays relevant deploy hash for subsequent querying.

```
nctl-do-auction-withdraw

nctl-do-auction-withdraw net=1 node=1 amount=1000000 payment=200000 gas=10 user=1  (same as above)

nctl-do-auction-withdraw amount=2000000
```

### nctl-do-auction-delegate net={X:-1} node={Y:-1} amount={A:-1000000} payment={P:-200000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from user U, a Proof-Of-Stake **delegate** bid for amount A (motes).  Displays relevant deploy hash for subsequent querying.

```
nctl-do-auction-delegate

nctl-do-auction-delegate net=1 node=1 amount=1000000 payment=200000 gas=10 user=1  (same as above)

nctl-do-auction-delegate amount=2000000
```

### nctl-do-auction-undelegate net={X:-1} node={Y:-1} amount={A:-1000000} payment={P:-200000} gas={G:-10} user={U:-1}

Dispatches to node Y in network X from user U, a Proof-Of-Stake **undelegate** bid for amount A (motes).  Displays relevant deploy hash for subsequent querying.

```
nctl-do-auction-undelegate

nctl-do-auction-undelegate net=1 node=1 amount=1000000 payment=200000 gas=10 user=1  (same as above)

nctl-do-auction-undelegate amount=2000000
```

## ERC-20 Token Standard 

### nctl-erc20-do-approve owner={X:-1} spender={Y:-1} amount={Z:-1000000000} 

Allows a user (the spender) to withdraw upto Z tokens from the account of a token holder (the owner).  

```
nctl-erc20-do-approve 

nctl-erc20-do-approve owner=1 spender=2 amount=1000000000  (same as above)

nctl-erc20-do-approve owner=2 spender=4 amount=4400000000  (same as above)
```

### nctl-erc20-do-fund-users amount={X:-1000000000} 

Transfers from the installed ERC-20 smart contract X tokens to each of the test user accounts.  

```
nctl-erc20-do-fund-users 

nctl-erc20-do-fund-users amount=1000000000  (same as above)

nctl-erc20-do-fund-users amount=4440000000  
```

### nctl-erc20-do-install name={X:-Acme Token} symbol={Y:-Acme} supply={Z:-1e33}  

Installs the ERC0-20 smart contract under the network's faucet account.  

NOTE : The compiled wasm file must have been previously moved to the test network's bin folder (e.g. `casper-node/utils/nctl/assets/net-1/bin`).  For further information refer to the network's smart contract SDK.

```
nctl-erc20-do-install 

nctl-erc20-do-install name="Acme Token" symbol="ACME" supply=1000000000000000000000000000000000  (same as above)

nctl-erc20-do-install name="Casper Labs" symbol="CSPR" supply=1000000000000000000000000000
```

### nctl-erc20-do-transfer user={X:-1} amount={Y:-1000000000} 

Transfers Y tokens to user X.  

```
nctl-erc20-do-transfer 

nctl-erc20-do-transfer user=1 amount=1000000000  (same as above)

nctl-erc20-do-transfer user=2 amount=4440000000  
```

### nctl-erc20-do-transfer-from from={X:-1} to={Y:-1} amount={Z:-1000000000} 

Transfers Z tokens from user X to user Y.  

```
nctl-erc20-do-transfer-from 

nctl-erc20-do-transfer-from from=1 to=2 amount=1000000000  (same as above)

nctl-erc20-do-transfer-from from=2 to=5 amount=4440000000  
```

### nctl-erc20-view-allowances spender={X:-2}   

Returns amount which spender is allowed to withdraw from other users.  

```
nctl-erc20-view-allowances 

nctl-erc20-view-allowances user=1  (same as above)

nctl-erc20-view-allowances user=3  
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
