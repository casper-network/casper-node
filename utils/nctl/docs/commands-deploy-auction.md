# NCTL Deploy Commands - Proof Of Stake Auction

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
