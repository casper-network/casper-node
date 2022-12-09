# global-state-update-gen

If the network experiences a catastrophic failure, it might become impossible to make changes to the global state required for fixing the situation via normal channels (i.e. executing deploys on the network), and we might instead need to resort to social consensus outside the blockchain and applying the changes manually. This tool facilitates generating files specifying such changes, which can then be applied during an emergency upgrade.

The tool consists of 1 main subcommand and 3 legacy subcommands:
- `generic` - a generic update based on a config file,
- `validators` (legacy) - updating the set of validators on the network,
- `balances` (legacy) - performing some transfers between accounts,
- `system-contract-registry` (legacy) - this was a single-use subcommand intended to introduce some changes to the system structures in the global state that couldn't be made otherwise.

## A detailed description of the subcommands

All subcommands share 3 parameters:

- `-h`, `--help` - prints help information about the subcommand,
- `-d`, `--data-dir` - path to the data directory of a node, containing its storage and global state database,
- `-s`, `--state-hash` - the root hash of the global state to be used as the base for the update - usually the state root hash from the last block before the planned upgrade.

### `generic`

Usage: `global-state-update-gen generic -d DATA-DIRECTORY -s STATE-ROOT-HASH CONFIG-FILE`

The config file should be a TOML file, which can contain the following values:

```toml
# can be true or false, optional, false if not present; more detailed description below
# *must* be listed before all [[accounts]] and [[transfers]] entries
only_listed_validators = false

# multiple [[accounts]] definitions are possible
[[accounts]]
public_key = "..." # the public key of the account owner
balance = "..."    # account balance, in motes (optional)

# if the account is supposed to be validator, define the section below
[accounts.validator]
bonded_amount = "..."      # the staked amount for this account, in motes
delegation_rate = ...      # the delegation rate for this validator (optional)

# define delegators as entries in accounts.validator.delegators
# multiple definitions per validator are possible
[[accounts.validator.delegators]]
public_key = "..."         # the delegator's public key
delegated_amount = "..."   # the amount delegated to the validator, in motes

# multiple [[transfers]] definitions are possible
[[transfers]]
from = "account-hash-..." # the account hash to transfer funds from
to = "account-hash-..."   # the account hash to transfer funds to
amount = "..."            # the amount to be transferred, in motes
```

The `[[accounts]]` definitions control the balances and stakes of accounts on the network. It is possible to change the set of validators using these definitions, by changing the staked amounts.

For every such definition, if the `balance` key is present, the balance of the account will be updated. The account will be created if it didn't exist previously. If the `balance` key is not present, the pre-existing balance (if any) will be left untouched.

Updating the validator properties (stake, delegators) behaves differently based on the value of `only_listed_validators`. If it is false, the existing list of validators is treated as a base, and validator properties are modified based on the entries in the config. If the `validator` key is present, the stake and delegators are set to the configured values. If it is not present, the pre-existing properties are left untouched.

If `only_listed_validators` is true, pre-existing validators are discarded, and only the accounts with non-zero stakes configured in the config file will be validators after the update. This option exists to match the behavior of the legacy `validators` subcommand and to cater to some use cases in testing.

So, for example, if the network has 100 validators and we want to only change the stake of a single one:
- with `only_listed_validators` set to false, we need only a single `[[accounts]]` entry for the validator we want to change,
- with `only_listed_validators` set to true, we need 100 `[[accounts]]` entries, one per each account that is supposed to be a validator after the upgrade.

On the other hand, replacing 100 validators with 5 different ones (a use case in testing setups) would require:
- just 5 entries for the new validators if `only_listed_validators` is true,
- 105 entries - 100 to remove the old validators, and 5 to add the new ones - if `only_listed_validators` is false.

The `[[transfers]]` definitions simply transfer funds from one account to another. Every definition requires a source account, a target account and an amount to be defined. If the source account doesn't contain enough funds for the transfer, it won't be executed. If the target account doesn't exist, it will be created.

**Note:** transfers are executed before the `[[accounts]]` definitions. This means that it is possible to overwrite the effects of a transfer if the source or target account is also specified among `[[accounts]]`.

After the transfers have been executed and account balances and stakes have been updated, the tool also updates the auction contract state in the form of the snapshot of validators sets for the next few eras, as well as bids and withdraws:
- bids of accounts with zero stake are set to empty, and bids of accounts with nonzero stake are created or updated to reflect the configured amounts,
- if `only_listed_validators` is true, any bid larger than the smallest stake among the new set of validators is reset to zero,
- the withdraws of validators that are being removed from the set are cancelled.

The tool also takes care to update the total supply in the network to reflect the changes in balances resulting from the configured modifications to the state.

### Legacy commands

#### `validators`

Usage: `global-state-update-gen validators -d DATA-DIRECTORY -s STATE-ROOT-HASH -v VALIDATOR-KEY,STAKE,BALANCE -v VALIDATOR-KEY,STAKE,BALANCE ...`

Apart from the common `-d` and `-s` parameters, the subcommand has one additional parameter, `-v` or `--validator`. Multiple such parameters can be supplied. Also note that the third field, `BALANCE`, is optional (ie., the definition can be just `-v VALIDATOR-KEY,STAKE`).

Every `-v` instance configures a single validator to be included in the set after the upgrade. A `-v KEY,STAKE,BALANCE` corresponds to an `[[accounts]]` entry in the config file:

```toml
[[accounts]]
public_key = "KEY"
balance = "BALANCE"

[accounts.validator]
bonded_amount = "STAKE"
```

The command as a whole works just like a config file with only `[[accounts]]` entries and `only_listed_validators` set to `true`.

#### `balances`

Usage: `global-state-update-gen balances -d DATA_DIRECTORY -s STATE_ROOT_HASH -f FROM-ACCOUNT -t TO-ACCOUNT -a AMOUNT`

This functions exactly like a config file with just a single transfer configured:

```toml
[[transfers]]
from = "FROM-ACCOUNT"
to = "TO-ACCOUNT"
amount = "AMOUNT"
```

#### `system-contract-registry`

Usage: `global-state-update-gen system-contract-registry -d DATA_DIRECTORY -s STATE_ROOT_HASH`

This subcommand doesn't take any additional parameters. It adds a registry of system contracts to the global state, based either on the data contained within the state (if `-s` is present), or based on the protocol data in storage (if `-s` is not present).

It has been used to add the registry to the global state during the upgrade to 1.4.0 and will most likely never be needed again.
