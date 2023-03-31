//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesI32, fromBytesString, toBytesMap } from "../../../../contract_as/assembly/bytesrepr";
import { Key } from "../../../../contract_as/assembly/key";
import { CLValue, CLType, CLTypeTag } from "../../../../contract_as/assembly/clvalue";
import { Pair } from "../../../../contract_as/assembly/pair";
import { RuntimeArgs } from "../../../../contract_as/assembly/runtime_args";

const COUNT_KEY = "count";
const COUNTER_INC = "counter_inc";
const COUNTER_GET = "counter_get";
const COUNTER_KEY = "counter";
const CONTRACT_VERSION_KEY = "version";

function counterGet(contractHash: StaticArray<u8>): i32 {
  let bytes = CL.callContract(contractHash, COUNTER_GET, new RuntimeArgs());
  if (bytes !== null) {
    return fromBytesI32(bytes).unwrap();
  }
  else {
    Error.fromErrorCode(ErrorCode.Formatting).revert();
    return <i32>unreachable();
  }
}

function counterInc(contractHash: StaticArray<u8>): void {
  CL.callContract(contractHash, COUNTER_INC, new RuntimeArgs());
}

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
