# Spin up net.
nctl-assets-setup
nctl-start

# Await for the faucet to be funded.
sleep 10.0
nctl-view-faucet-account-balance

# Fund user accounts.
nctl-do-transfer user=1 transfers=10
nctl-do-transfer user=2 transfers=10
nctl-do-transfer user=3 transfers=10
nctl-do-transfer user=4 transfers=10
nctl-do-transfer user=5 transfers=10
sleep 10.0
nctl-view-user-account-balance user=1
nctl-view-user-account-balance user=2
nctl-view-user-account-balance user=3
nctl-view-user-account-balance user=4
nctl-view-user-account-balance user=5

# Install ERC20 contract under the faucet account.
nctl-contracts-erc20-do-install name=Broadleaf symbol=BLF supply=1000000000000000000000000000000000
sleep 2.0
nctl-view-faucet-account
nctl-contracts-erc20-view-details

# Fund test user accounts.
nctl-contracts-erc20-do-fund-users
sleep 5.0
nctl-contracts-erc20-view-balances
