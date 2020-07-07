import {Error} from "../../../../contract_as/assembly/error";

export function call(): void {
    Error.fromUserError(100).revert();
}
