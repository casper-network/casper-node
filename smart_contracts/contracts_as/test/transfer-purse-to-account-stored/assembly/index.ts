//@ts-nocheck
import * as CL from "../../../../contract_as/assembly";
import {Error, ErrorCode} from "../../../../contract_as/assembly/error";
import {U512} from "../../../../contract_as/assembly/bignum";
import {getMainPurse} from "../../../../contract_as/assembly/account";
import {Key} from "../../../../contract_as/assembly/key";
import {putKey} from "../../../../contract_as/assembly";
import {CLValue, CLType, CLTypeTag} from "../../../../contract_as/assembly/clvalue";
import {URef} from "../../../../contract_as/assembly/uref";
import {toBytesMap} from "../../../../contract_as/assembly/bytesrepr";
import * as TransferPurseToAccount  from "../../transfer-purse-to-account/assembly";
import {getBalance, transferFromPurseToAccount, TransferredTo} from "../../../../contract_as/assembly/purse";
import {Pair} from "../../../../contract_as/assembly/pair";

const ENTRY_FUNCTION_NAME = "transfer";
const PACKAGE_HASH_KEY_NAME = "transfer_purse_to_account";
const HASH_KEY_NAME = "transfer_purse_to_account_hash";
const ACCESS_KEY_NAME = "transfer_purse_to_account_access";
const ARG_0_NAME = "target_account_addr";
const ARG_1_NAME = "amount";


enum CustomError{
    MissingAmountArg = 1,
    InvalidAmountArg = 2,
    MissingDestinationAccountArg = 3,
    UnableToGetBalance = 103
}

export function transfer(): void {
    TransferPurseToAccount.delegate();
}

export function call(): void {
    let entryPoints = new CL.EntryPoints();
    let args = new Array<Pair<String, CLType>>();
    args.push(new Pair(ARG_0_NAME, CLType.byteArray(32)));
    args.push(new Pair(ARG_1_NAME, new CLType(CLTypeTag.U512)));

    let entryPoint = new CL.EntryPoint(ENTRY_FUNCTION_NAME, args, new CLType(CLTypeTag.Unit), new CL.PublicAccess(), CL.EntryPointType.Session);
    entryPoints.addEntryPoint(entryPoint);
    let newResult = CL.newContract(
      entryPoints,
      null,
      PACKAGE_HASH_KEY_NAME,
      ACCESS_KEY_NAME,
    );
    CL.putKey(HASH_KEY_NAME, Key.fromHash(newResult.contractHash));
}