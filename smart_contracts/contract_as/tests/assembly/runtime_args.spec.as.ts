import { hex2bin } from "../utils/helpers";
import { checkArraysEqual, checkItemsEqual } from "../../assembly/utils";
import { typedToArray } from "../../assembly/utils";
import { RuntimeArgs } from "../../assembly/runtime_args";
import { Pair } from "../../assembly/pair";

import { CLValue } from "../../assembly/clvalue";
import { U512 } from "../../assembly/bignum";


export function testRuntimeArgs(): bool {
    // Source:
    //
    // ```
    // let args = runtime_args! {
    //     "arg1" => 42u64,
    //     "arg2" => "Hello, world!",
    //     "arg3" => U512::from(123456789),
    // };
    // ```
    const truth = hex2bin("030000000400000061726731080000002a00000000000000050400000061726732110000000d00000048656c6c6f2c20776f726c64210a0400000061726733050000000415cd5b0708");
    let runtimeArgs = RuntimeArgs.fromArray([
        new Pair("arg1", CLValue.fromU64(42)),
        new Pair("arg2", CLValue.fromString("Hello, world!")),
        new Pair("arg3", CLValue.fromU512(U512.fromU64(123456789))),
    ]);
    let bytes = runtimeArgs.toBytes();
    return checkArraysEqual<u8, Array<u8>>(truth, bytes);
}

export function testRuntimeArgs_Empty(): bool {
    // Source:
    //
    // ```
    // let args = runtime_args! {
    //     "arg1" => 42u64,
    //     "arg2" => "Hello, world!",
    //     "arg3" => U512::from(123456789),
    // };
    // ```
    const truth = hex2bin("00000000");

    let runtimeArgs = new RuntimeArgs();
    let bytes = runtimeArgs.toBytes();
    return checkArraysEqual<u8, Array<u8>>(truth, bytes);
}
