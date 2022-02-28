//@ts-nocheck
import {getMainPurse} from "../../../../contract_as/assembly/account";
import * as CL from "../../../../contract_as/assembly";
import {Error} from "../../../../contract_as/assembly/error";
import {URef, AccessRights} from "../../../../contract_as/assembly/uref";

const ARG_PURSE = "purse";

enum CustomError {
  MissingExpectedMainPurseArg = 86,
  InvalidExpectedMainPurseArg = 97,
  EqualityAssertionFailed = 139
}

export function call(): void {
  let expectedMainPurseArg = CL.getNamedArg(ARG_PURSE);
  let purseResult = URef.fromBytes(expectedMainPurseArg);
  if (purseResult === null){
    Error.fromUserError(<u16>CustomError.InvalidExpectedMainPurseArg).revert();
    return;
  }
  const expectedMainPurse = purseResult.value;
  const actualMainPurse = getMainPurse().withAccessRights(AccessRights.READ_ADD);

  if (<URef>expectedMainPurse != <URef>actualMainPurse)
    Error.fromUserError(<u16>CustomError.EqualityAssertionFailed).revert();
}
