import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {fromBytesString, fromBytesI32} from "../../../../contract_as/assembly/bytesrepr";
import {arrayToTyped} from "../../../../contract_as/assembly/utils";
import {Key} from "../../../../contract_as/assembly/key"
import {addAssociatedKey, AddKeyFailure, ActionType, setActionThreshold, SetThresholdFailure} from "../../../../contract_as/assembly/account";

const ARG_KEY_MANAGEMENT_THRESHOLD = "key_management_threshold";
const ARG_DEPLOY_THRESHOLD = "deploy_threshold";

export function call(): void {
  let keyManagementThresholdBytes = CL.getNamedArg(ARG_KEY_MANAGEMENT_THRESHOLD);
  let keyManagementThreshold = keyManagementThresholdBytes[0];

  let deployThresholdBytes = CL.getNamedArg(ARG_DEPLOY_THRESHOLD);
  let deployThreshold = deployThresholdBytes[0];

  if (keyManagementThreshold != 0) {
    if (setActionThreshold(ActionType.KeyManagement, keyManagementThreshold) != SetThresholdFailure.Ok) {
      // TODO: Create standard Error from those enum values
      Error.fromUserError(4464 + 1).revert();
    }
  }
  if (deployThreshold != 0) {
    if (setActionThreshold(ActionType.Deployment, deployThreshold) != SetThresholdFailure.Ok) {
      Error.fromUserError(4464).revert();
      return;
    }
  }

}
