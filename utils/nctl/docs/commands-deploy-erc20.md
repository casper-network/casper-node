# NCTL Deploy Commands - ERC-20

### nctl-erc20-approve owner={X:-1} spender={Y:-1} amount={Z:-1000000000} 

Allows a user (the spender) to withdraw up to Z tokens from the account of a token holder (the owner).  

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
