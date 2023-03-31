import * as CL from "../../../../contract_as/assembly";
import { getKey } from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { CLValue } from "../../../../contract_as/assembly/clvalue";
import { U512 } from "../../../../contract_as/assembly/bignum";
import { fromBytesString } from "../../../../contract_as/assembly/bytesrepr";
import { URef } from "../../../../contract_as/assembly/uref";
import { KeyVariant } from "../../../../contract_as/assembly/key";
import { transferFromPurseToPurse, transferFromPurseToPursePassthrough } from "../../../../contract_as/assembly/purse";
import { RuntimeArgs } from "../../../../contract_as/assembly/runtime_args";
import { Pair } from "../../../../contract_as/assembly/pair";

const GET_PAYMENT_PURSE = "get_payment_purse";
const SET_REFUND_PURSE = "set_refund_purse";
const ARG_AMOUNT = "amount";
const ARG_PURSE = "purse";
const ARG_PURSE_NAME = "purse_name";

function fromBytesStringFast(bytes: StaticArray<u8>): String {
  const lengthPrefix = load<u32>(changetype<usize>(bytes));
  return String.UTF8.decodeUnsafe(changetype<usize>(bytes) + 4, lengthPrefix, false);
}

function getPurseURef(): StaticArray<u8> {
  let purseNameBytes = CL.getNamedArg(ARG_PURSE_NAME);
  let purseKeyBytes = CL.getKeyPassthrough(purseNameBytes);
  return StaticArray.fromArray(purseKeyBytes.slice(1));
}

export function call(): void {
  let maybePurseURef = getPurseURef();
  if (!maybePurseURef) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }

  let amountBytes = CL.getNamedArg(ARG_AMOUNT);

  let handlePayment = CL.getSystemContract(CL.SystemContract.HandlePayment);

  // Get Payment Purse
  let paymentPurseOutput = CL.callContract(handlePayment, GET_PAYMENT_PURSE, null);
  if (paymentPurseOutput === null) {
    Error.fromErrorCode(ErrorCode.InvalidPurse).revert();
    return;
  }

  let error = transferFromPurseToPursePassthrough(
    maybePurseURef,
    paymentPurseOutput,
    amountBytes,
  );
  if (error !== null) {
    error.revert();
    return;
  }
}
