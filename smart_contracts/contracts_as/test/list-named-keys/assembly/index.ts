//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {fromBytesMap, fromBytesString} from "../../../../contract_as/assembly/bytesrepr";
import {Key} from "../../../../contract_as/assembly/key";
import {checkItemsEqual} from "../../../../contract_as/assembly/utils";

const ARG_INITIAL_NAMED_KEYS = "initial_named_args";
const ARG_NEW_NAMED_KEYS = "new_named_keys";

enum CustomError {
  MissingInitialNamedKeys = 0,
  InvalidInitialNamedKeys = 1,
  MissingNewNamedKeys = 2,
  InvalidNewNamedKeys = 3,
  MissingActualNamedKeys = 4464,
  MismatchedKeys = 4505,
}

export function call(): void {
  let expectedInitialNamedKeysBytes = CL.getNamedArg(ARG_INITIAL_NAMED_KEYS);

  const mapResult = fromBytesMap<String, Key>(
    expectedInitialNamedKeysBytes,
    fromBytesString,
    Key.fromBytes
  );
  if (mapResult.hasError()) {
    Error.fromUserError(<u16>CustomError.InvalidInitialNamedKeys).revert();
    return;
  }
  let expectedInitialNamedKeys = mapResult.value;


  let actualNamedKeys = CL.listNamedKeys();
  if (actualNamedKeys === null) {
    Error.fromUserError(<u16>CustomError.MissingActualNamedKeys).revert();
    return;
  }


  if (!checkItemsEqual(expectedInitialNamedKeys, actualNamedKeys)) {
    Error.fromUserError(<u16>CustomError.MismatchedKeys).revert();
    return;
  }

  let newNamedKeysBytes = CL.getNamedArg(ARG_NEW_NAMED_KEYS);
  const mapResult2 = fromBytesMap<String, Key>(
    newNamedKeysBytes,
    fromBytesString,
    Key.fromBytes
  );
  if (mapResult2.hasError()) {
    Error.fromUserError(<u16>CustomError.InvalidNewNamedKeys).revert();
    return;
  }
  let newNamedKeys = mapResult2.value;

  let expectedNamedKeys = expectedInitialNamedKeys;

  for (let i = 0; i < newNamedKeys.length; i++) {
    const namedKey = newNamedKeys[i];
    CL.putKey(namedKey.first, namedKey.second);
    expectedNamedKeys.push(namedKey);

    const actualNamedKeys = CL.listNamedKeys();
    assert(checkItemsEqual(expectedNamedKeys, actualNamedKeys));
  }


  let allKeyNames = new Array<String>();
  for (let i = 0; i < expectedNamedKeys.length; i++) {
    allKeyNames.push(expectedNamedKeys[i].first);
  }

  for (let i = 0; i < allKeyNames.length; i++) {
    CL.removeKey(allKeyNames[i]);

    // TODO: remove on an ordered map, or reconsider giving Map a try with Map.remove
    let removed = false;
    for (let j = 0; j < expectedNamedKeys.length; j++) {
      if (expectedNamedKeys[j].first == allKeyNames[i]) {
        expectedNamedKeys.splice(j, 1);
        removed = true;
        break;
      }
    }

    assert(removed);

    const actualNamedKeys = CL.listNamedKeys();
    assert(checkItemsEqual(expectedNamedKeys, actualNamedKeys));
  }
}
