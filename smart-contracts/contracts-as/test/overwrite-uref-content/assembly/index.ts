import * as CL from "../../../../contract-as/assembly";
import {Error, ErrorCode} from "../../../../contract-as/assembly/error";
import {AccessRights, URef} from "../../../../contract-as/assembly/uref";
import {Key} from "../../../../contract-as/assembly/key";
import {CLValue} from "../../../../contract-as/assembly/clvalue";

const ARG_CONTRACT_UREF = "contract_uref";
const REPLACEMENT_DATA = "bawitdaba";

export function call(): void {
  let urefBytes = CL.getNamedArg(ARG_CONTRACT_UREF);
  let urefResult = URef.fromBytes(urefBytes);
  if (urefResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
    return;
  }
  let uref = urefResult.value;

  if (uref.isValid() == false){
    Error.fromUserError(1).revert();
    return;
  }

  let elevatedUref = new URef(
    uref.getBytes(),
    AccessRights.READ_ADD_WRITE
  );

  let forgedKey = Key.fromURef(elevatedUref);

  let value = CLValue.fromString(REPLACEMENT_DATA);

  forgedKey.write(value);
}
