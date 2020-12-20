# NCTL Viewing Account Information

## Overview

As part of the process of setting up a network, NCTL creates various on-chain accounts.  Such accounts relate either to node operators (i.e. validators), to a network level faucet, or to tests user accounts.  Each account is initialised with a valid key pair and an on-chain CSPR balance.  NCTL greatly simplifies account management in a test setting.

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

### nctl-view-faucet-main-purse-uref net={X:-1}

Displays main purse unforgeable reference of faucet associated with network X.

```
nctl-view-faucet-main-purse-uref

nctl-view-faucet-main-purse-uref net=1  (same as above)
```

### nctl-view-faucet-secret-key-path net={X:-1} user={Y:-all}

Displays path to secret key in PEM format of the faucet associated with network X.

```
nctl-view-faucet-secret-key-path

nctl-view-faucet-secret-key-path net=1   (same as above)

nctl-view-faucet-secret-key-path net=2 
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

### nctl-view-user-main-purse-uref net={X:-1} user={Y:-all}

Displays main purse unforgeable reference of user Y.

```
nctl-view-user-main-purse-uref

nctl-view-user-main-purse-uref net=1 user=all (same as above)

nctl-view-user-main-purse-uref net=1 user=3 
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

### nctl-view-validator-main-purse-uref net={X:-1} node={Y:-all}

Displays main purse unforgeable reference of validator Y.

```
nctl-view-validator-main-purse-uref

nctl-view-validator-main-purse-uref net=1 user=all (same as above)

nctl-view-validator-main-purse-uref net=1 user=3 
```

### nctl-view-validator-secret-key-path net={X:-1} node={Y:-all}

Displays path to secret key in PEM format of validator Y.

```
nctl-view-validator-secret-key-path

nctl-view-validator-secret-key-path net=1 node=all  (same as above)

nctl-view-validator-secret-key-path net=1 node=1
```
