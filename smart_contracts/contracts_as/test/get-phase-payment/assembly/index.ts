// The entry file of your WebAssembly module.
import * as CL from "../../../../contract_as/assembly";
import * as externals from "../../../../contract_as/assembly/externals";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { U512 } from "../../../../contract_as/assembly/bignum";
import { URef } from "../../../../contract_as/assembly/uref";
import { RuntimeArgs } from "../../../../contract_as/assembly/runtime_args";
import { getMainPurse } from "../../../../contract_as/assembly/account";
import { transferFromPurseToPurse } from "../../../../contract_as/assembly/purse";

const ARG_PHASE = "phase";
const ARG_AMOUNT = "amount";
const HANDLE_PAYMENT_ACTION = "get_payment_purse";

function standardPayment(amount: U512): void {
  let handlePayment = CL.getSystemContract(CL.SystemContract.HandlePayment);

  let mainPurse = getMainPurse();

  let outputURefBytes = CL.callContract(handlePayment, HANDLE_PAYMENT_ACTION, null);
  if (outputURefBytes === null) {
    Error.fromErrorCode(ErrorCode.InvalidPurse).revert();
    return;
  }
  let paymentPurseResult = URef.fromBytes(outputURefBytes);
  if (paymentPurseResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidPurse).revert();
    return;
  }
  let paymentPurse = paymentPurseResult.value;

  let error = transferFromPurseToPurse(
    mainPurse,
    paymentPurse,
    amount,
  );
  if (error !== null) {
    error.revert();
    return;
  }
}

export function call(): void {
  const phaseBytes = CL.getNamedArg(ARG_PHASE);
  if (phaseBytes.length != 1) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }

  const phase = <CL.Phase>phaseBytes[0];

  const amountBytes = CL.getNamedArg(ARG_AMOUNT);
  let amountResult = U512.fromBytes(amountBytes);
  if (amountResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }
  let amount = amountResult.value;

  const caller = CL.getPhase();
  if (<u8>phase != <u8>caller) {
    Error.fromErrorCode(ErrorCode.Unhandled).revert();
    return;
  }

  standardPayment(amount);
}
