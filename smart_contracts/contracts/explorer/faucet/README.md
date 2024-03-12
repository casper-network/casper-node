# Faucet Contract

The Faucet is a contract that is designed to allow users to create a new account or to allow existing users to fund their account easily.

## Install Session
The install session is responsible for creating the faucet's stored contract package and setting up a few named keys in the account used to perform the install. The installer performs the following actions.

1. Takes the `id` parameter and uses it to keep track of the faucet being set up. As an example, if the operator installing the faucet passes `1337` as the `id` named argument to the installer session, it will create a named key called `faucet_1337` for the account used to call the install session.
1. Calls the stored faucet's `init` entry point to initialize a new purse that will be used to fund the faucet. This purse is stored under the named keys of the account used to install the faucet. The `init` entrypoint also initializes the faucet's state and writes to its named keys.
1. Funds the faucet's purse with the amount of motes declared in the `amount` runtime argument.


## Set Variables

> NOTE: Before the faucet can be called, the `set_variables` entrypoint must be called.
>
This is a list of the required runtime arguments for calling `set_variables`
* `available_amount: U512` - The total amount available for distribution each interval.
* `distributions_per_interval: u64` - The maximum number of distributions to be made each interval.
* `time_interval: u64` - The amount of time in milliseconds that must pass before the available amount is replenished.


You can adjust the faucet's distribution rate by modifying the variables. If the faucet has distributed a total of `available_amount` in one `time_interval`, then no more token will be available to distribute until `last_distribution_at + time_interval < blocktime`. However, the installer of this contract is not rate limited and may continue to distribute funds from the faucet freely.

## Calling the Faucet

The faucet will calculate a distribution amount as a ratio of the available amount per interval to the max distribution amount per interval. As an example, if the installer sets the available amount per interval to `100_000_000` and the max distributions per interval to `2`. When an existing user calls the faucet, `50_000_000` motes will be distributed to the caller. If a second user calls the faucet, they will also receive `50_000_000` motes. The remaining amount will now be `0` tokens. If a third user calls the faucet then they will not receive any token.
After an interval passes after then last user was funded, the available amount will be replenished.

`distributions_per_interval`, `available_amount`, `time_interval` and `max_distributions_per_interval`
must be set and must be a number greater than `0` for the contract to run properly.
If you try to invoke the contract before these variables are set, then you'll get an error.

### Costs by Entry Point

| feature                  | cost             |
|--------------------------|------------------|
| faucet install           | `88_383_544_080` |
| faucet set variables     | `110_689_570`    |
| faucet call by installer | `2_776_789_130`  |
| faucet call by user      | `2_618_699_930`  |
