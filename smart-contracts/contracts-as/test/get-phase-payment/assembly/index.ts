// The entry file of your WebAssembly module.
import * as CL from "../../../../contract-as/assembly";
import {Error, ErrorCode} from "../../../../contract-as/assembly/error";
import {U512} from "../../../../contract-as/assembly/bignum";
import {URef} from "../../../../contract-as/assembly/uref";
import {RuntimeArgs} from "../../../../contract-as/assembly/runtime_args";
import {getMainPurse} from "../../../../contract-as/assembly/account";
import {transferFromPurseToPurse} from "../../../../contract-as/assembly/purse";

const ARG_PHASE = "phase";
const ARG_AMOUNT = "amount";
const POS_ACTION = "get_payment_purse";

function standardPayment(amount: U512): void {
  let proofOfStake = CL.getSystemContract(CL.SystemContract.ProofOfStake);

  let mainPurse = getMainPurse();

  let output = CL.callContract(proofOfStake, POS_ACTION, new RuntimeArgs());

  let paymentPurseResult = URef.fromBytes(output);
  if (paymentPurseResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidPurse).revert();
    return;
  }
  let paymentPurse = paymentPurseResult.value;

  let ret = transferFromPurseToPurse(
    mainPurse,
    paymentPurse,
    amount,
  );
  if (ret > 0) {
    Error.fromErrorCode(ErrorCode.Transfer).revert();
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

  const caller = CL.getPhase();
  assert(<u8>phase == <u8>caller);

  standardPayment(U512.fromU64(10000000));
}
