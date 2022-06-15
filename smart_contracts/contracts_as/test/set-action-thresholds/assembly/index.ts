import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { fromBytesString, fromBytesI32 } from "../../../../contract_as/assembly/bytesrepr";
import { arrayToTyped } from "../../../../contract_as/assembly/utils";
import { Key } from "../../../../contract_as/assembly/key"
import { addAssociatedKey, AddKeyFailure, ActionType, setActionThreshold, SetThresholdFailure } from "../../../../contract_as/assembly/account";

const ARG_KEY_MANAGEMENT_THRESHOLD = "key_management_threshold";
const ARG_DEPLOY_THRESHOLD = "deploy_threshold";

function handleError(error: SetThresholdFailure): void {
  switch (error) {
    case SetThresholdFailure.Ok:
      break;
    case SetThresholdFailure.KeyManagementThreshold:
      Error.fromErrorCode(ErrorCode.KeyManagementThreshold).revert();
      break;
    case SetThresholdFailure.DeploymentThreshold:
      Error.fromErrorCode(ErrorCode.DeploymentThreshold).revert();
      break;
    case SetThresholdFailure.PermissionDeniedError:
      Error.fromErrorCode(ErrorCode.PermissionDenied).revert();
      break;
    case SetThresholdFailure.InsufficientTotalWeight:
      Error.fromErrorCode(ErrorCode.InsufficientTotalWeight).revert();
      break;
  }
}

export function call(): void {
  let keyManagementThresholdBytes = CL.getNamedArg(ARG_KEY_MANAGEMENT_THRESHOLD);
  let keyManagementThreshold = keyManagementThresholdBytes[0];

  let deployThresholdBytes = CL.getNamedArg(ARG_DEPLOY_THRESHOLD);
  let deployThreshold = deployThresholdBytes[0];

  if (keyManagementThreshold != 0) {
    const result1 = setActionThreshold(ActionType.KeyManagement, keyManagementThreshold);
    handleError(result1);
  }

  if (deployThreshold != 0) {
    const result2 = setActionThreshold(ActionType.Deployment, deployThreshold);
    handleError(result2);
  }
}
