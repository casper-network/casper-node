import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesString, fromBytesI32 } from "../../../../contract_as/assembly/bytesrepr";
import { arrayToTyped } from "../../../../contract_as/assembly/utils";
import { Key } from "../../../../contract_as/assembly/key"
import { addAssociatedKey, AddKeyFailure, ActionType, setActionThreshold, SetThresholdFailure } from "../../../../contract_as/assembly/account";

const ARG_KEY_MANAGEMENT_THRESHOLD = "key_management_threshold";
const ARG_DEPLOY_THRESHOLD = "deploy_threshold";

export function call(): void {
  let keyManagementThresholdBytes = CL.getNamedArg(ARG_KEY_MANAGEMENT_THRESHOLD);
  let keyManagementThreshold = keyManagementThresholdBytes[0];

  let deployThresholdBytes = CL.getNamedArg(ARG_DEPLOY_THRESHOLD);
  let deployThreshold = deployThresholdBytes[0];

  if (keyManagementThreshold != 0) {
    const result1 = setActionThreshold(ActionType.KeyManagement, keyManagementThreshold);
    switch (result1) {
      case SetThresholdFailure.Ok:
        break;
      case SetThresholdFailure.PermissionDeniedError:
        Error.fromErrorCode(ErrorCode.PermissionDenied).revert();
        break;
      default:
        // TODO: Create standard Error from those enum values
        Error.fromUserError(4464 + 1).revert();
        break;
    }
  }

  if (deployThreshold != 0) {
    const result2 = setActionThreshold(ActionType.Deployment, deployThreshold);
    switch (result2) {
      case SetThresholdFailure.Ok:
        break;
      case SetThresholdFailure.PermissionDeniedError:
        Error.fromErrorCode(ErrorCode.PermissionDenied).revert();
        break;
      default:
        // TODO: Create standard Error from those enum values
        Error.fromUserError(4464).revert();
        break;
    }
  }
}
