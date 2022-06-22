import * as CL from "../../../../contract_as/assembly";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { Key } from "../../../../contract_as/assembly/key";
import { fromBytesString } from "../../../../contract_as/assembly/bytesrepr";
import { U512 } from "../../../../contract_as/assembly/bignum";
import { CLValue } from "../../../../contract_as/assembly/clvalue";

const COMMAND_CREATE_UREF1 = "create-uref1";
const COMMAND_CREATE_UREF2 = "create-uref2";
const COMMAND_REMOVE_UREF1 = "remove-uref1";
const COMMAND_REMOVE_UREF2 = "remove-uref2";
const COMMAND_TEST_READ_UREF1 = "test-read-uref1";
const COMMAND_TEST_READ_UREF2 = "test-read-uref2";
const COMMAND_INCREASE_UREF2 = "increase-uref2";
const COMMAND_OVERWRITE_UREF2 = "overwrite-uref2";
const ARG_COMMAND = "command";

export function call(): void {
  let commandBytes = CL.getNamedArg(ARG_COMMAND);
  let commandResult = fromBytesString(commandBytes);
  if (commandResult.hasError()) {
    Error.fromErrorCode(ErrorCode.InvalidArgument);
    return;
  }
  let command = commandResult.value;

  if (command == COMMAND_CREATE_UREF1) {
    let helloWorldKey = Key.create(CLValue.fromString("Hello, world!"));
    if (!helloWorldKey) {
      Error.fromUserError(4464 + 1).revert();
      return;
    }
    CL.putKey("hello-world", <Key>helloWorldKey);
  }

  else if (command == COMMAND_CREATE_UREF2) {
    let newBigValueKey = Key.create(CLValue.fromU512(U512.MAX_VALUE));
    if (!newBigValueKey) {
      Error.fromUserError(4464 + 4).revert();
      return;
    }
    CL.putKey("big-value", <Key>newBigValueKey);
  }

  else if (command == COMMAND_REMOVE_UREF1) {
    CL.removeKey("hello-world");
  }

  else if (command == COMMAND_REMOVE_UREF2) {
    CL.removeKey("big-value");
  }

  else if (command == COMMAND_TEST_READ_UREF1) {
    let namedKeys = CL.listNamedKeys();
    // Read data hidden behind `URef1` uref
    namedKeys = CL.listNamedKeys();

    let helloWorld: String = "";
    for (let i = 0; i < namedKeys.length; i++) {
      if (namedKeys[i].first == "hello-world") {
        let bytes = namedKeys[i].second.read();
        if (bytes === null) {
          Error.fromUserError(4464 + 1000 + <u16>i).revert();
          return;
        }

        let bytesString = fromBytesString(bytes);
        if (bytesString.hasError()) {
          Error.fromUserError(4464 + 2000 + <u16>i).revert();
          return;
        }
        helloWorld = bytesString.value;
      }
    }

    if (helloWorld != "Hello, world!") {
      Error.fromUserError(4464 + 6).revert();
      return;
    }

    // Read data through dedicated FFI function
    let uref1 = CL.getKey("hello-world");
    if (!uref1) {
      Error.fromErrorCode(ErrorCode.GetKey).revert();
      return;
    }
    let uref1Bytes = uref1.read();
    if (uref1Bytes === null) {
      Error.fromUserError(4464 + 7).revert();
      return;
    }
    let uref1Str = fromBytesString(uref1Bytes);
    if (uref1Str.hasError()) {
      Error.fromUserError(4464 + 8).revert();
      return;
    }
    if (uref1Str.value != "Hello, world!") {
      Error.fromUserError(4464 + 9).revert();
      return;
    }
  }

  else if (command == COMMAND_TEST_READ_UREF2) {
    // Get the big value back
    let bigValueKey = CL.getKey("big-value");
    if (!bigValueKey) {
      Error.fromErrorCode(ErrorCode.GetKey).revert();
      return;
    }
    let bigValueBytes = bigValueKey.read();
    if (!bigValueBytes) {
      Error.fromUserError(4464 + 12).revert();
      return;
    }
    let bigValue = U512.fromBytes(bigValueBytes);
    if (bigValue.hasError()) {
      Error.fromUserError(4464 + 13).revert();
      return;
    }

    if (bigValue.value != U512.MAX_VALUE) {
      Error.fromUserError(4464 + 14).revert();
      return;
    }
  }

  else if (command == COMMAND_INCREASE_UREF2) {
    // Get the big value back
    let bigValueKey = CL.getKey("big-value");
    if (!bigValueKey) {
      Error.fromErrorCode(ErrorCode.GetKey).revert();
      return;
    }
    // Increase by 1
    bigValueKey.add(CLValue.fromU512(U512.fromU64(1)));
    let newBigValueBytes = bigValueKey.read();
    if (!newBigValueBytes) {
      Error.fromUserError(4464 + 15).revert();
      return;
    }
    let newBigValue = U512.fromBytes(newBigValueBytes);
    if (newBigValue.hasError()) {
      Error.fromUserError(4464 + 16).revert();
      return;
    }
    if (newBigValue.value != U512.MIN_VALUE) {
      Error.fromUserError(4464 + 17).revert();
      return;
    }
  }

  else if (command == COMMAND_OVERWRITE_UREF2) {
    // Get the big value back
    let bigValueKey = CL.getKey("big-value");
    if (!bigValueKey) {
      Error.fromErrorCode(ErrorCode.GetKey).revert();
      return;
    }
    // I can overwrite some data under the pointer
    bigValueKey.write(CLValue.fromU512(U512.fromU64(123456789)));

    let newBigValueBytes = bigValueKey.read();
    if (!newBigValueBytes) {
      Error.fromUserError(4464 + 18).revert();
      return;
    }
    let newBigValue = U512.fromBytes(newBigValueBytes);
    if (newBigValue.hasError()) {
      Error.fromUserError(4464 + 19).revert();
      return;
    }
    if (newBigValue.value != U512.fromU64(123456789)) {
      Error.fromUserError(4464 + 20).revert();
      return;
    }
  }

  else {
    Error.fromErrorCode(ErrorCode.InvalidArgument).revert();
  }
}
