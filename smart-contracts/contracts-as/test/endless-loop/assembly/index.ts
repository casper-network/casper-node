import {getMainPurse} from "../../../../contract-as/assembly/account";

export function call(): void {
  while(true){
    getMainPurse();
  }
}
