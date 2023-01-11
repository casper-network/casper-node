//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { Result, fromBytesMap, fromBytesArray } from "../../../../contract_as/assembly/bytesrepr";
import { AccountHash, Key } from "../../../../contract_as/assembly/key";
import { listAuthorizationKeys } from "../../../../contract_as/assembly/account";
import { checkItemsEqual } from "../../../../contract_as/assembly/utils";

const ARG_EXPECTED_AUTHORIZATION_KEYS = "expected_authorization_keys";

enum UserError {
  AssertionFailure = 0,
}

export function call(): void {
  const authorizationKeys = listAuthorizationKeys();
  const expectedAuthorizedKeysBytes = CL.getNamedArg(ARG_EXPECTED_AUTHORIZATION_KEYS);
  if (expectedAuthorizedKeysBytes === null) {
    Error.fromErrorCode(ErrorCode.MissingArgument).revert();
    return;
  }

  let fromBytesAccountHash = (bytes: StaticArray<u8>): Result<AccountHash> => AccountHash.fromBytes(bytes.slice(0))
  let expectedAuthorizedKeys = fromBytesArray<AccountHash>(expectedAuthorizedKeysBytes, fromBytesAccountHash).unwrap();
  expectedAuthorizedKeys.sort();

  if (authorizationKeys.length != expectedAuthorizedKeys.length) {
    Error.fromUserError(UserError.AssertionFailure as u16).revert();
  }

  for (let i = 0; i < authorizationKeys.length; i++) {
    if (authorizationKeys[i] != expectedAuthorizedKeys[i]) {
      Error.fromUserError(UserError.AssertionFailure as u16).revert();
    }
  }
}
