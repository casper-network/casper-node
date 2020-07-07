import {Error} from "../../../../contract-as/assembly/error";

export function call(): void {
    Error.fromUserError(100).revert();
}
