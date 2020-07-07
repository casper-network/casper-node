import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";

const ARG_PHASE = "phase";

export function call(): void {
  const phaseBytes = CL.getNamedArg(ARG_PHASE);
  if (phaseBytes.length != 1) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }

  const phase = <CL.Phase>phaseBytes[0];

  const caller = CL.getPhase();
  assert(<u8>phase == <u8>caller);
}
