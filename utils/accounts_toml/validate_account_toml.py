import argparse
from pathlib import Path
try:
    import toml
except ImportError:
    print("toml package not available.")
    print("Install with `python3 -m pip install toml`.")
    exit(1)
from validate import is_valid_key


def parse_args():

    def file_path(path):
        if not Path(path).is_file():
            raise argparse.ArgumentTypeError(f'{path} is not a valid file.')
        if not Path(path).exists():
            raise argparse.ArgumentTypeError(f'{path} does not exist.')
        return Path(path)

    parser = argparse.ArgumentParser(description='Convert csv files to accounts.toml.')
    parser.add_argument('accounts_toml_path', type=file_path, help="Path to accounts.toml file")
    return parser.parse_args()


def is_int(value):
    try:
        _ = int(value)
    except ValueError:
        return False
    else:
        return True


def is_valid(toml_obj):
    is_good = True
    for key in toml_obj.keys():
        if key not in ("accounts", "delegators"):
            print(f"ERR: Unknown root level group: {key}")
            is_good = False
    accounts = toml_obj.get("accounts", None)
    delegators = toml_obj.get("delegators", None)
    if accounts is None:
        print("ERR: No accounts found.")
        is_good = False
    if delegators is None:
        print("WARN: No delegators found.")

    account_keys = []
    validator_keys = []
    total_amt = 0
    for account in accounts:
        account_key = account.get("public_key", None)
        account_balance = account.get("balance", None)
        validator = account.get("validator", None)
        if validator is None:
            account_bonded_amount, account_delegation_rate = None, None
        else:
            account_bonded_amount = validator.get("bonded_amount", None)
            account_delegation_rate = validator.get("delegation_rate", None)

        if account_key is None:
            print("ERR: Missing accounts.public_key")
            is_good = False
        elif not is_valid_key(account_key):
            print(f"ERR: Invalid accounts.public_key: {account_key}")
            is_good = False
        elif account_key in account_keys:
            print(f"ERR: Duplicated account_key: {account_key}")
            is_good = False
        else:
            account_keys.append(account_key)

        if account_balance is None:
            print("ERR: Missing accounts.balance")
            is_good = False
        elif not is_int(account_balance):
            print(f"ERR: Invalid accounts.balance: {account_balance} for {account_key}")
            is_good = False
        else:
            total_amt += int(account_balance)

        if account_bonded_amount is not None or account_delegation_rate is not None:
            validator_keys.append(account_key)

            if account_bonded_amount is None:
                print(f"ERR: No bonded_amount with delegation_rate for {account_key}")
                is_good = False
            elif not is_int(account_bonded_amount):
                print(f"ERR: Invalid bonded_amount for {account_key}")
                is_good = False
            else:
                total_amt += int(account_bonded_amount)

            if account_delegation_rate is None:
                print(f"ERR: No delegation_rate with bonded_amount for {account_key}")
                is_good = False
            elif not is_int(account_delegation_rate):
                print(f"ERR: Invalid delegation_rate for {account_key}")
                is_good = False
            elif int(account_delegation_rate) < 0 or int(account_delegation_rate) > 100:
                print(f"ERR: delegation_rate not between 0 and 100 for {account_key}")
                is_good = False

    delegator_keys = []
    for delegator in delegators:
        validator_key = delegator.get("validator_public_key", None)
        delegator_key = delegator.get("delegator_public_key", None)
        balance = delegator.get("balance", None)
        delegated_amount = delegator.get("delegated_amount", None)

        if validator_key is None:
            print(f"ERR: Missing validator_key for delegator_key: {delegator_key}")
            is_good = False
        elif not is_valid_key(validator_key):
            print(f"ERR: Invalid validator_key: {validator_key}")
            is_good = False
        elif validator_key not in validator_keys:
            print(f"ERR: Cannot delegate to unknown validator_key: {validator_key}")
            is_good = False

        if delegator_key is None:
            print(f"ERR: Missing delegator_key with validator_key: {validator_key}")
            is_good = False
        elif not is_valid_key(delegator_key):
            print(f"ERR: Invalid delegator_key: {delegator_key}")
            is_good = False
        elif delegator_key in delegator_keys:
            print(f"ERR: Duplicate delegator_key: {delegator_key}")
            is_good = False
        else:
            delegator_keys.append(delegator_key)

        if balance is None:
            print(f"ERR: No balance with delegator_key: {delegator_key}")
            is_good = False
        elif not is_int(balance):
            print(f"ERR: Invalid balance with delegator_key: {delegator_key}")
            is_good = False
        else:
            total_amt += int(balance)

        if delegated_amount is None:
            print(f"ERR: No delegated_amount with delegator_key: {delegator_key}")
            is_good = False
        elif not is_int(delegated_amount):
            print(f"ERR: Invalid delegated_amount with delegator_key: {delegator_key}")
            is_good = False
        else:
            total_amt += int(delegated_amount)

    if is_good:
        print(f"Total Amt: {total_amt}")
    return is_good


if __name__ == '__main__':
    args = parse_args()
    print(args)

    toml_obj = toml.load(args.accounts_toml_path)
    if is_valid(toml_obj):
        print(f"{args.accounts_toml_path} is valid.")

