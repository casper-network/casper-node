# NCTL Viewing Account Information

## Overview

As part of the process of setting up a network, NCTL creates various on-chain accounts.  Such accounts relate either to node operators (i.e. validators), to a network level faucet, or to tests user accounts.  Each account is initialised with a valid key pair and an on-chain CSPR balance.  NCTL greatly simplifies account management in a test setting.

## Viewing faucet information

### nctl-view-faucet-account

Displays faucet account information.

```
nctl-view-faucet-account
```

## Viewing user information

### nctl-view-user-account user={X:-1}

Displays on-chain user account information.

```
nctl-view-user-account

nctl-view-user-account user=1  (same as above)

nctl-view-user-account user=3  
```

## Viewing validator information

### nctl-view-validator-account node={X:-1}

Displays on-chain validator account information.

```
nctl-view-validator-account

nctl-view-validator-account node=1  (same as above)

nctl-view-validator-account node=4  
```
