# NCTL Viewing Account Information

## Overview

As part of the process of setting up a network, NCTL creates various on-chain accounts.  Such accounts relate either to node operators (i.e. validators), to a network level faucet, or to tests user accounts.  Each account is initialised with a valid key pair and an on-chain CSPR balance.  NCTL greatly simplifies account management in a test setting.

## Viewing faucet information

### nctl-view-faucet-account

Displays on-chain faucet account information.

```
nctl-view-faucet-account
```

### nctl-view-faucet-account-balance

Displays faucet main purse balance.

```
nctl-view-faucet-account-balance
```

### nctl-view-faucet-account-hash

Displays faucet account hash.

```
nctl-view-faucet-account-hash
```

### nctl-view-faucet-account-key

Displays faucet public key in HEX format.

```
nctl-view-faucet-account-key
```

### nctl-view-faucet-main-purse-uref

Displays faucet main purse unforgeable reference.

```
nctl-view-faucet-main-purse-uref
```

### nctl-view-faucet-secret-key-path 

Displays path to secret key in PEM format of faucet.

```
nctl-view-faucet-secret-key-path
```

## Viewing user information

### nctl-view-user-account user={X:-1}

Displays on-chain user account information.

```
nctl-view-user-account

nctl-view-user-account user=1  (same as above)

nctl-view-user-account user=3  
```

### nctl-view-user-account-balance user={X:-all}

Displays main purse balance of user X.

```
nctl-view-user-account-balance

nctl-view-user-account-balance user=all  (same as above)

nctl-view-user-account-balance user=1  (same as above)
```

### nctl-view-user-account-key user={X:-all}

Displays public key in HEX format of user X.

```
nctl-view-user-account-key

nctl-view-user-account-key user=all  (same as above)

nctl-view-user-account-key user=3 
```

### nctl-view-user-account-hash user={X:-all}

Displays account hash of user Y.

```
nctl-view-user-account-hash

nctl-view-user-account-hash user=all  (same as above)

nctl-view-user-account-hash user=3  
```

### nctl-view-user-main-purse-uref user={X:-all}

Displays main purse unforgeable reference of user X.

```
nctl-view-user-main-purse-uref

nctl-view-user-main-purse-uref user=all (same as above)

nctl-view-user-main-purse-uref user=3 
```

### nctl-view-user-secret-key-path user={X:-all}

Displays path to secret key in PEM format of user X.

```
nctl-view-user-secret-key-path

nctl-view-user-secret-key-path user=all  (same as above)

nctl-view-user-secret-key-path user=3
```

## Viewing validator information

### nctl-view-validator-account node={X:-1}

Displays on-chain validator account information.

```
nctl-view-validator-account

nctl-view-validator-account node=1  (same as above)

nctl-view-validator-account node=4  
```

### nctl-view-validator-account-balance node={X:-all}

Displays main purse balance of validator X.

```
nctl-view-validator-account-balance

nctl-view-validator-account-balance node=all  (same as above)

nctl-view-validator-account-balance node=4
```

### nctl-view-validator-account-key node={X:-all}

Displays public key in HEX format of validator X.

```
nctl-view-validator-account-key

nctl-view-validator-account-key node=all  (same as above)

nctl-view-validator-account-key node=4  
```

### nctl-view-validator-account-hash node={X:-all}

Displays account hash of validator X.

```
nctl-view-validator-account-hash

nctl-view-validator-account-hash node=all  (same as above)

nctl-view-validator-account-hash node=4  
```

### nctl-view-validator-main-purse-uref node={X:-all}

Displays main purse unforgeable reference of validator X.

```
nctl-view-validator-main-purse-uref

nctl-view-validator-main-purse-uref user=all (same as above)

nctl-view-validator-main-purse-uref user=3 
```

### nctl-view-validator-secret-key-path node={X:-all}

Displays path to secret key in PEM format of validator X.

```
nctl-view-validator-secret-key-path

nctl-view-validator-secret-key-path node=all  (same as above)

nctl-view-validator-secret-key-path node=4
```
