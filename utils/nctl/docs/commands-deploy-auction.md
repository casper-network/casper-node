# NCTL Deploy Commands - Proof Of Stake Auction

### nctl-auction-bid amount={X:-1000000} bidder={Y:-6} rate={Z:-125} 

Dispatches on behalf of validator Y, a Proof-Of-Stake auction bid **submission** deploy for amount X (motes) with a delegation rate of Z.  Displays relevant deploy hash for subsequent querying.

```
nctl-auction-bid

nctl-auction-bid bidder=6 amount=6000000000000000 rate=125   (same as above)

nctl-auction-bid bidder=7 amount=7000000000000000 rate=250
```

### nctl-auction-withdraw amount={X:-1000000} bidder={Y:-6} 

Dispatches on behalf of validator Y, a Proof-Of-Stake auction bid **withdrawal** deploy for amount X (motes).  Displays relevant deploy hash for subsequent querying.

```
nctl-auction-withdraw

nctl-auction-withdraw bidder=6 amount=6000000000000000 (same as above)

nctl-auction-withdraw bidder=7 amount=7000000000000000 
```

### nctl-auction-delegate amount={X:-1000000} delegator={Y:-1} validator={Z:-1} 

Dispatches on behalf of user Y, a Proof-Of-Stake **delegate** bid for amount X (motes) nominating validator Y.  Displays relevant deploy hash for subsequent querying.

```
nctl-auction-delegate

nctl-auction-delegate amount=1000000 delegator=1 validator=1  (same as above)

nctl-auction-delegate amount=2000000 delegator=3 validator=4
```

### nctl-auction-undelegate amount={X:-1000000} delegator={Y:-1} validator={Z:-1}

Dispatches on behalf of user Y, a Proof-Of-Stake **undelegate** bid for amount X (motes) un-nominating validator Y.  Displays relevant deploy hash for subsequent querying.

```
nctl-auction-undelegate

nctl-auction-undelegate amount=1000000 delegator=1 validator=1  (same as above)

nctl-auction-undelegate amount=2000000 delegator=3 validator=4
```
