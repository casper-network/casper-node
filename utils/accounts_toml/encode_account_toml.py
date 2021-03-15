import argparse
from pathlib import Path
try:
    import toml
except ImportError:
    print("toml package not available.")
    print("Install with `python3 -m pip install toml`.")
    exit(1)

# This file takes two .csv files and encodes into one accounts.toml.
# accounts.csv    <public_key_hex>,<balance>,<bonded amt>,<delegation rate>
# delegators.csv  <delegator_key_hex>,<balance>,<validator_key_hex>,<delegation_amt>


def parse_args():

    def file_path(path):
        if not Path(path).is_file():
            raise argparse.ArgumentTypeError(f'{path} is not a valid file.')
        if not Path(path).exists():
            raise argparse.ArgumentTypeError(f'{path} does not exist.')
        return Path(path)

    def valid_dir(path):
        if not Path(path).is_dir():
            raise argparse.ArgumentTypeError(f'{path} is not a valid directory.')
        if not Path(path).exists():
            raise argparse.ArgumentTypeError(f'{path} does not exist.')
        return Path(path)

    parser = argparse.ArgumentParser(description='Convert csv files to accounts.toml.')
    parser.add_argument('-a', type=file_path, help="Path to accounts.csv file", required=True)
    parser.add_argument('-d', type=file_path, help="Path to delegators.csv file", required=False)

    return parser.parse_args()


def load_accounts_csv(csv_path: Path):
    accounts = []
    for line in csv_path.read_text().strip().splitlines():
        columns = line.split(',')
        if len(columns) != 4:
            print(f"Expected 4 pieces of data: {line}")
            exit(1)
        account_dict = {"public_key": columns[0],
                        "balance": columns[1]}
        if columns[2] != "" or columns[3] != "":
            account_dict["validator"] = {"bonded_amount": columns[2],
                                         "delegation_rate": int(columns[3])}
        accounts.append(account_dict)
    return accounts


def load_delegators_csv(csv_path: Path):
    delegators = []
    for line in csv_path.read_text().strip().splitlines():
        columns = line.split(',')
        if len(columns) != 4:
            print(f"Expected 4 pieces of data: {line}")
            exit(1)
        delegators.append({"delegator_public_key": columns[0],
                           "balance": columns[1],
                           "validator_public_key": columns[2],
                           "delegated_amount": columns[3]})
    return delegators


if __name__ == '__main__':
    args = parse_args()

    doc_dict = {"accounts": load_accounts_csv(args.a)}
    if args.d:
        doc_dict["delegators"] = load_delegators_csv(args.d)

    toml_output = toml.dumps(doc_dict)
    print(toml_output)
