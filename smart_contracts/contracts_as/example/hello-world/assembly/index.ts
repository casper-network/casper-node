import * as CL from "../../../../contract_as/assembly";
import { CLValue } from "../../../../contract_as/assembly/clvalue";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesString, fromBytesI32 } from "../../../../contract_as/assembly/bytesrepr";
import { arrayToTyped } from "../../../../contract_as/assembly/utils";
import { Key } from "../../../../contract_as/assembly/key"
import { addAssociatedKey, AddKeyFailure, ActionType, setActionThreshold, SetThresholdFailure } from "../../../../contract_as/assembly/account";

const KEY = "special_value"
const ARG_MESSAGE = "message";

function store(value: String): void {
  const valueRef = CLValue.fromString(value);
  const valueKey = Key.create(valueRef);
  if (!valueKey) {
    Error.fromUserError(0).revert();
    return;
  }
  CL.putKey(KEY, valueKey);
}

export function call(): void {
  let valueBytes = CL.getNamedArg(ARG_MESSAGE);
  let value = fromBytesString(valueBytes).unwrap();
  store(value);
}
