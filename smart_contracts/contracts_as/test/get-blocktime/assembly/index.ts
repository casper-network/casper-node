import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {fromBytesU64} from "../../../../contract_as/assembly/bytesrepr";

const ARG_KNOWN_BLOCK_TIME = "known_block_time";

export function call(): void {
  const knownBlockTimeBytes = CL.getNamedArg(ARG_KNOWN_BLOCK_TIME);
  const knownBlockTime = fromBytesU64(knownBlockTimeBytes);
  if (knownBlockTime.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }

  const blockTime = CL.getBlockTime();
  assert(blockTime == knownBlockTime.value);
}
