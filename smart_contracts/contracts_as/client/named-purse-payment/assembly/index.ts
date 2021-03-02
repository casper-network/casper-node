import * as CL from "../../../../contract_as/assembly";
import {getKey} from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {CLValue} from "../../../../contract_as/assembly/clvalue";
import {U512} from "../../../../contract_as/assembly/bignum";
import {fromBytesString} from "../../../../contract_as/assembly/bytesrepr";
import {URef} from "../../../../contract_as/assembly/uref";
import {KeyVariant} from "../../../../contract_as/assembly/key";
import {transferFromPurseToPurse} from "../../../../contract_as/assembly/purse";
import {RuntimeArgs} from "../../../../contract_as/assembly/runtime_args";
import {Pair} from "../../../../contract_as/assembly/pair";

const GET_PAYMENT_PURSE = "get_payment_purse";
const SET_REFUND_PURSE= "set_refund_purse";
const ARG_AMOUNT = "amount";
const ARG_PURSE = "purse";
const ARG_PURSE_NAME = "purse_name";

function getPurseURef(): URef | null{
  let purseNameBytes = CL.getNamedArg(ARG_PURSE_NAME);

  let purseName = fromBytesString(purseNameBytes);
  if (purseName.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument);
    return null;
  }

  let purseKey = getKey(purseName.value);
  if (purseKey === null) {
    Error.fromErrorCode(ErrorCode.InvalidArgument);
    return null;
  }

  if (purseKey.variant != KeyVariant.UREF_ID) {
    Error.fromErrorCode(ErrorCode.UnexpectedKeyVariant);
    return null;
  }

  let purse = <URef>purseKey.uref;

  return purse;
}

export function call(): void {
  let maybePurseURef = getPurseURef();
  if (maybePurseURef === null) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }

  let purseURef = <URef>maybePurseURef;

  let amountBytes = CL.getNamedArg(ARG_AMOUNT);
  let amountResult = U512.fromBytes(amountBytes);
  if (amountResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }
  let amount = amountResult.value;

  let handlePayment = CL.getSystemContract(CL.SystemContract.HandlePayment);

  // Get Payment Purse
  let paymentPurseOutput = CL.callContract(handlePayment, GET_PAYMENT_PURSE, new RuntimeArgs());

  let paymentPurseResult = URef.fromBytes(paymentPurseOutput);
  if (paymentPurseResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidPurse).revert();
    return;
  }
  let paymentPurse = paymentPurseResult.value;

  // Set Refund Purse
  let runtimeArgs = RuntimeArgs.fromArray([
    new Pair(ARG_PURSE, CLValue.fromURef(purseURef)),
  ]);
  CL.callContract(handlePayment, SET_REFUND_PURSE, runtimeArgs);

  let error = transferFromPurseToPurse(
    purseURef,
    paymentPurse,
    amount,
  );
  if (error !== null) {
    error.revert();
    return;
  }
}
