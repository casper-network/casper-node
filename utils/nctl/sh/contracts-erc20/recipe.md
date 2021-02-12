# Spin up net.
nctl-assets-setup
nctl-start

# Await for the faucet to be funded.
sleep 30.0
nctl-view-faucet-accounts

# Fund user accounts.
nctl-do-fund-users
sleep 10.0

# Install ERC20 contract under the faucet account.
nctl-erc20-install name=Broadleaf symbol=BLF supply=1000000000000000000000000000000000
sleep 5.0

# View contract details.
nctl-erc20-view-details

# Fund user accounts with ERC-20 tokens.
nctl-erc20-fund-users
sleep 5.0

# View ERC-20 token user balances.
nctl-erc20-view-balances
