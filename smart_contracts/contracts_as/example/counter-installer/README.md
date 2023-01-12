# Counter Contract Installation

This example code will install a simple counter using AssemblyScript. Instructions for getting started on Casper using AssemblyScript can be found in our [Getting Started with AssemblyScript](https://docs.casperlabs.io/dapp-dev-guide/writing-contracts/assembly-script/) guide.

## Session Code

Installing a smart contract to global state on a Casper Network requires the use of session code. This example code uses AssemblyScript to send a Wasm-bearing [Deploy](https://docs.casperlabs.io/glossary/D/#deploy) containing the contract to be installed.


### Step 1. Configuring the Index.ts File

The first section of *assembly/index.ts* outlines the necessary dependencies to include and the constants to establish.

1. Begin with `//@ts-nocheck` to disable semantic checks.

2. Establish imported dependencies from other packages.

3. Establish constants for use within the installed contract.

```typescript

// Importing necessary aspects of external packages.
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesI32, fromBytesString, toBytesMap } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { Pair } from "../../../../contract_as/assembly/pair";

// Establishing constants for use within the installing contract.
const COUNT_KEY = "count";
const COUNTER_INC = "counter_inc";
const COUNTER_GET = "counter_get";
const COUNTER_KEY = "counter";
const CONTRACT_VERSION_KEY = "version";

```

### Step 2. Defining the Contract Entry Points

Each entry point that we later establish will require an associated function to perform the desired action. In the next section of *index.ts* we define these functions to perform the actions of incrementing the counter and viewing the current counter value.

```typescript

// Creating a function that will be used by the incrementing entry point.
export function counter_inc(): void {
  let countKey = CL.getKey(COUNT_KEY);
  if (!countKey) {
    Error.fromErrorCode(ErrorCode.MissingKey).revert();
    return;
  }

  if (!countKey.isURef()) {
    Error.fromErrorCode(ErrorCode.UnexpectedKeyVariant).revert();
    return;
  }

  const one = CLValue.fromI32(1);

  countKey.add(one);
}

// Creating a function that will be used by the value retrieval entry point.
export function counter_get(): void {
  let countKey = CL.getKey(COUNT_KEY);

  if (!countKey) {
    Error.fromErrorCode(ErrorCode.MissingKey).revert();
    return;
  }

  if (!countKey.isURef()) {
    Error.fromErrorCode(ErrorCode.UnexpectedKeyVariant).revert();
    return;
  }
  let countData = countKey.read();
  if (!countData) {
    Error.fromErrorCode(ErrorCode.ValueNotFound).revert();
    return;
  }
  let value = fromBytesI32(<StaticArray<u8>>countData).unwrap();
  CL.ret(CLValue.fromI32(value));
}

```

### Step 3. Defining the `Call` Function

We will also define the `Call` function that will start the code execution and perform the contract installation. This function initializes the contract by setting the counter to 0 and creating `NamedKeys` to be passed to the contract. Specifically, it creates the keys `counter_package_name` and `counter_access_uref` which will persist within the contract's context.

Further, it establishes the entry points `entryPointInc` and `entryPointGet` tied to their respective functions as defined above. Finally, it creates a key with the `ContractHash` to allow access to the installed contract code.

```typescript

export function call(): void {
  // Create entry points to get the counter value and to increment the counter by 1.
  let counterEntryPoints = new CL.EntryPoints();

  let entryPointInc = new CL.EntryPoint(COUNTER_INC, new Array(), new CLType(CLTypeTag.Unit), new CL.PublicAccess(), CL.EntryPointType.Contract);
  counterEntryPoints.addEntryPoint(entryPointInc);
  let entryPointGet = new CL.EntryPoint(COUNTER_GET, new Array(), new CLType(CLTypeTag.I32), new CL.PublicAccess(), CL.EntryPointType.Contract);
  counterEntryPoints.addEntryPoint(entryPointGet);

  // Initialize counter to 0.
  let counterLocalKey = Key.create(CLValue.fromI32(0));
  if (!counterLocalKey) {
    Error.fromUserError(0).revert();
    return;
  }

  // Create initial named keys of the contract.
  let counterNamedKeys = new Array<Pair<String, Key>>();
  counterNamedKeys.push(new Pair<String, Key>(COUNT_KEY, counterLocalKey));

  const result = CL.newContract(
    counterEntryPoints,
    counterNamedKeys,
    "counter_package_name",
    "counter_access_uref",
  );

  // To create a locked contract instead, use new_locked_contract and throw away the contract version returned
  // const result = CL.newLockedContract(counterEntryPoints, counterNamedKeys, "counter_package_name", "counter_access_uref");

  // The current version of the contract will be reachable through named keys
  const versionURef = Key.create(CLValue.fromI32(result.contractVersion));
  if (!versionURef) {
    return;
  }
  CL.putKey(CONTRACT_VERSION_KEY, versionURef);

  // Hash of the installed contract will be reachable through named keys
  CL.putKey(COUNTER_KEY, Key.fromHash(result.contractHash));
}

```
