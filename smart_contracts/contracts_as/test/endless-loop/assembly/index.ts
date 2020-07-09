import {getMainPurse} from "../../../../contract_as/assembly/account";

export function call(): void {
  while(true){
    getMainPurse();
  }
}
