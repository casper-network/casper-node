# NCTL Viewing Chain Information

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

### nctl-view-chain-block net={X:-1} node={Y:-1} block={Z:-LATEST}

Displays details of block Z at node Y on network Z.  If block Z is undefined then it defaults to latest block processed at node Y.

```
nctl-view-chain-block

nctl-view-chain-block net=1 node=1 (same as above)

nctl-view-chain-block net=1 node=3 block=A_BLOCK_HASH 
```

### nctl-view-chain-block-transfers net={X:-1} node={Y:-1} block={Z:-LATEST}

Displays details of transfers within block Z at node Y on network Z.  If block Z is undefined then it defaults to latest block processed at node Y.

```
nctl-view-chain-block-transfers

nctl-view-chain-block-transfers net=1 node=1 (same as above)

nctl-view-chain-block-transfers net=1 node=3 block=A_BLOCK_HASH
```

### nctl-view-chain-deploy net={X:-1} node={Y:-1} deploy=Z

Displays details of deploy Z at node Y on network Z.

```
nctl-view-chain-deploy deploy=A_DEPLOY_HASH

nctl-view-chain-deploy net=1 node=1 deploy=A_DEPLOY_HASH (same as above)
```

### nctl-view-chain-era net={X:-1} node={Y:-all}

Displays era ID at node Y on network Z.

```
nctl-view-chain-era

nctl-view-chain-era net=1 node=all (same as above)

nctl-view-chain-era net=1 node=3
```

### nctl-view-chain-era-info net={X:-1} node={Y:-1}

Displays era information at node Y on network Z.

```
nctl-view-chain-era-info

nctl-view-chain-era-info net=1 node=1 (same as above)

nctl-view-chain-era-info net=1 node=3
```

### nctl-view-chain-height net={X:-1} node={Y:-all}

Displays block height ID at node Y on network Z.

```
nctl-view-chain-height

nctl-view-chain-height net=1 node=all (same as above)

nctl-view-chain-height net=1 node=3
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
