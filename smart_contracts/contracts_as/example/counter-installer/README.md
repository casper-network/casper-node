# Counter Contract Session Code

This example code will call our [previously installed Counter Contract](https://github.com/casper-network/casper-node/tree/dev/smart_contracts/contracts_as/example/counter-installer) using AssemblyScript. Instructions for getting started on Casper using AssemblyScript can be found in our [Getting Started with AssemblyScript](https://docs.casperlabs.io/dapp-dev-guide/writing-contracts/assembly-script/) guide.

## Calling Contract Code

Previously, we installed a simple counter smart contract that contains an incrementing variable. We defined two entry points, `entryPointInc` and `entryPointGet`. We also defined the `call` function that initialized the contract. This constitutes the contract code that we will now call using session code.

### Step 1. Configuring the Index.ts File

The first section of *assembly/index.ts* outlines the necessary dependencies to include and the constants to establish.

1. Begin with `//@ts-nocheck` to disable semantic checks.

2. Establish imported dependencies from other packages.

3. Establish constants for use within the installed contract.

```typescript

//@ts-nocheck
// Importing necessary components from other packages.
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesI32, fromBytesString, toBytesMap } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { Pair } from "../../../../contract_as/assembly/pair";
import { RuntimeArgs } from "../../../../contract_as/assembly/runtime_args";


// Establishing constants for use within our session code.
const COUNT_KEY = "count";
const COUNTER_INC = "counter_inc";
const COUNTER_GET = "counter_get";
const COUNTER_KEY = "counter";
const CONTRACT_VERSION_KEY = "version";

```

### Step 2. Defining Functions for Interacting with the Smart Contract

While our `call` function will execute the session code and interact with the installed counter contract, we will need to define the actions our `call` function will take.

* `counterGet` retrieves the current value of the counter.

* `counterInc` increments the counter value by 1.

```typescript

function counterGet(contractHash: Uint8Array): i32 {
  let bytes = CL.callContract(contractHash, COUNTER_GET, new RuntimeArgs());
  return fromBytesI32(bytes).unwrap();
}

function counterInc(contractHash: Uint8Array): void {
  CL.callContract(contractHash, COUNTER_INC, new RuntimeArgs());
}

```

### Step 3. Defining the `Call` Function

The `call` function serves to execute the session code, calling the counter contract. To do so, it uses the `COUNTER_KEY` constant, which should contain the `contractHash` of the counter contract.

It then reads the current value of the counter, increments the value by 1 and then reads the updated value of the counter. Finally, it observes the difference between the first value observed (`currentCounterValue`) and the new value (`newCounterValue`). If the the difference is not equal to 1, it will cause an error.

```typescript

export function call(): void {
  // Read the Counter smart contract's ContractHash.
  let contractHashKey = CL.getKey(COUNTER_KEY);
  if (!contractHashKey) {
    Error.fromUserError(66).revert();
    return;
  }
  let contractHash = contractHashKey.hash;
  if (!contractHash) {
    Error.fromUserError(66).revert();
    return;
  }

  // Call Counter to get the current value.
  let currentCounterValue = counterGet(contractHash);

  // Call Counter to increment the value.
  counterInc(contractHash);

  // Call Counter to get the new value.
  let newCounterValue = counterGet(contractHash);

  // Expect counter to increment by one.
  if (newCounterValue - currentCounterValue != 1) {
    Error.fromUserError(67).revert();
  }
}

```