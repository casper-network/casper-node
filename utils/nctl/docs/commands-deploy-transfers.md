# NCTL Deploy Commands - Simple Transfers

### nctl-transfer net={X:-1} node={Y:-1} payment={P:-1000000000} gas={G:-10} transfers={T:-100} interval={I:-0.01} user={U:-1}

Dispatches to node Y in network X, T wasmless transfers from network faucet to user U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-transfer

nctl-transfer net=1 node=1 payment=1000000000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-transfer transfers=10000 interval=0.001
```

Note: has a synonym: `nctl-transfer-native`

### nctl-do-transfer-wasm net={X:-1} node={Y:-1} payment={P:-1000000000} gas={G:-10} transfers={T:-100} interval={I:-0.01} user={U:-1}

Dispatches to node Y in network X, T wasm based transfers from network faucet to user U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-do-transfer-wasm

nctl-do-transfer-wasm net=1 node=1 payment=1000000000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-do-transfer-wasm transfers=10000 interval=0.001
```
