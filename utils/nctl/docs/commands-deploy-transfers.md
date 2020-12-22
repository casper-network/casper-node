# NCTL Deploy Commands - Simple Transfers

## Default parameters

Most of the below commands accept the following default parameters:

- `net={X:-1}`
- `node={Y:-1}` 
- `payment={P:-1000000000}` 
- `gas={G:-10}`

### nctl-transfer-native amount={A:-1000000000} transfers={T:-100} interval={I:-0.01} user={U:-1} 

Dispatches to node Y in network X, T native transfers from network faucet to user U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-transfer-native

nctl-transfer-native net=1 node=1 payment=1000000000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-transfer-native transfers=10000 interval=0.001
```

Note: has a synonym: `nctl-transfer`

### nctl-transfer-native-batch-dispatch net={X:-1} node={Y:-1} interval={Z:-0.01} 

Dispatches to node Y in network X previously prepared native transfers at an interval of Z seconds.

```
nctl-transfer-native-batch-dispatch

nctl-transfer-native-batch-dispatch net=1 node=1 interval=0.01  (same as above)

nctl-transfer-native-batch-dispatch net=1 node=3 interval=0.001
```

### nctl-transfer-native-batch-prepare amount={A:-1000000000} count={C:-10} size={S:-10}

Writes to file system C batches of signed native transfers, with S deploys per user per batch.

```
nctl-transfer-native-batch-prepare

nctl-transfer-native-batch-prepare amount=1000000000 count=10 size=10 net=1 node=1 payment=1000000000 gas=10  (same as above)

nctl-transfer-native-batch-prepare transfers=10000 interval=0.001
```

### nctl-transfer-wasm amount={A:-1000000000} transfers={T:-100} interval={I:-0.01} user={U:-1} 

Dispatches to node Y in network X, T wasm based transfers from network faucet to user U.  If node=all then transfers are dispatched to nodes in a round-robin fashion.

```
nctl-transfer-wasm

nctl-transfer-wasm net=1 node=1 payment=1000000000 gas=10 transfers=100 interval=0.01 user=1  (same as above)

nctl-transfer-wasm transfers=10000 interval=0.001
```

### nctl-transfer-wasm-batch-dispatch net={X:-1} node={Y:-1} interval={Z:-0.01} 

Dispatches to node Y in network X previously prepared wasm transfers at an interval of Z seconds.

```
nctl-transfer-wasm-batch-dispatch

nctl-transfer-wasm-batch-dispatch net=1 node=1 interval=0.01  (same as above)

nctl-transfer-wasm-batch-dispatch net=1 node=3 interval=0.001
```

### nctl-transfer-wasm-batch-prepare amount={A:-1000000000} count={C:-10} size={S:-10}

Writes to file system C batches of signed wasm transfers, with S deploys per user per batch.

```
nctl-transfer-wasm-batch-prepare

nctl-transfer-wasm-batch-prepare amount=1000000000 count=10 size=10 net=1 node=1 payment=1000000000 gas=10  (same as above)

nctl-transfer-wasm-batch-prepare transfers=10000 interval=0.001
```
