# NCTL Viewing Chain Information

### nctl-view-chain-account account-key=X root-hash={Y:-LATEST}

Displays result of performing a state query by account key X at state root hash Y.  If state root hash is undefined then it defaults to latest.

```
nctl-view-chain-account account-key=AN_ACCOUNT_KEY

nctl-view-chain-account account-key=AN_ACCOUNT_KEY root-hash=A_STATE_ROOT_HASH
```

### nctl-view-chain-auction-info

Displays Proof of Stake auction contract information.

```
nctl-view-chain-auction-info
```

### nctl-view-chain-balance purse-uref=X root-hash={Y:-LATEST}

Displays balance of an account purse with uref X at state root hash Y.  If state root hash is undefined then it defaults to latest.

```
nctl-view-chain-balance purse-uref=A_PURSE_UREF

nctl-view-chain-balance purse-uref=A_PURSE_UREF root-hash=A_STATE_ROOT_HASH
```

### nctl-view-chain-balances

Displays balances of faucet, validator and user accounts (main purses).

```
nctl-view-chain-balances
```

### nctl-view-chain-block block={X:-LATEST}

Displays details of block X.  If block is undefined then it defaults to latest.

```
nctl-view-chain-block

nctl-view-chain-block block=A_BLOCK_HASH 
```

### nctl-view-chain-block-transfers block={X:-LATEST}

Displays details of transfers within block X.  If block is undefined then it defaults to latest.

```
nctl-view-chain-block-transfers

nctl-view-chain-block-transfers block=A_BLOCK_HASH
```

### nctl-view-chain-deploy deploy={X:-$DEPLOY_HASH}

Displays details of deploy X.  The hash of deploy X may be assigned as output of a previous command.

```
nctl-view-chain-deploy

nctl-view-chain-deploy deploy=A_DEPLOY_HASH
```

### nctl-view-chain-era node={X:-all}

Displays a chain's era at node X.

```
nctl-view-chain-era

nctl-view-chain-era node=all (same as above)

nctl-view-chain-era node=3
```

### nctl-view-chain-era-info node={X:-1}

Displays switch block era information at node X.

```
nctl-view-chain-era-info

nctl-view-chain-era-info node=1 (same as above)

nctl-view-chain-era-info node=3
```

### nctl-view-chain-height node={X:-all}

Displays a chain's block height at node X.

```
nctl-view-chain-height

nctl-view-chain-height node=all (same as above)

nctl-view-chain-height node=3
```

### nctl-view-chain-spec 

Displays a chain's chainspec toml file.

```
nctl-view-chain-spec 
```

### nctl-view-chain-spec-accounts  

Displays a chain's accounts.toml file.

```
nctl-view-chain-spec-accounts 
```

### nctl-view-chain-state-root-hash node={X:-all}

Displays a chain's state root hash.

```
nctl-view-chain-state-root-hash 

nctl-view-chain-state-root-hash node=all (same as above)

nctl-view-chain-state-root-hash node=3
```
