import {CLValue} from "./clvalue";
import { Pair } from "./pair";
import { toBytesU32, toBytesString } from "./bytesrepr";

/**
 * Implements a collection of runtime arguments.
 */
export class RuntimeArgs {
    constructor(public arguments: Pair<String, CLValue>[] = []) {}

    static fromArray(pairs: Pair<String, CLValue>[]): RuntimeArgs {
        return new RuntimeArgs(pairs);
    }

    toBytes(): Array<u8> {
        let bytes : u8[] = toBytesU32(<u32>this.arguments.length);
        let args = this.arguments;
        for (var i = 0; i < args.length; i++) {
            let pair = args[i];
            const argNameBytes = toBytesString(pair.first);
            bytes = bytes.concat(argNameBytes);
            const argValueBytes = pair.second.toBytes();
            bytes = bytes.concat(argValueBytes);
        }
        return bytes;
    }
}
