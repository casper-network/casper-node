import { getMainPurse } from "../../../../contract_as/assembly/account";
import { Key } from "../../../../contract_as/assembly/key";
import { Error, ErrorCode } from "../../../../contract_as/assembly/error";
import { toBytesArrayU8 } from "../../../../contract_as/assembly/bytesrepr";
import { CLValue, CLTypeTag, CLType } from "../../../../contract_as/assembly/clvalue";
import { U512 } from "../../../../contract_as/assembly/bignum";

export function call(): void {
  let key = Key.create(new CLValue(new Array<u8>(0), new CLType(CLTypeTag.Unit)));
  if (!key) {
    Error.fromErrorCode(ErrorCode.Unhandled).revert();
    return;
  }

  const bytes = new Array<u8>(4096);
  const serialized = toBytesArrayU8(bytes);

  while (true) {
    getMainPurse();
    key.write(new CLValue(serialized, CLType.list(new CLType(CLTypeTag.U8))));
  }
}
