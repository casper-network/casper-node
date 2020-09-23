# Charlie-5 Run Notes

## Start Time: 2020-09-24 17:00 UTC

## Changes

 - chainspec.toml and accounts.csv are no longer deployed with .deb package.
 - Fix to prevent equivocation when restarting nodes and catching up.
 - Improved log messages when detecting equivocation.
 - Improved log messages at era start.
 - Chainspec: `finality_threshold_percent = 12`
 - Chainspec: timestamp to 1600966800000
 - Added check for /etc/casper/accounts.csv and /etc/casper/chainspec.toml to systemd service since it is 
 no longer distributed with package.

## Testing

We have learned what we will from load testing this as we did not have expectations for more than mostly idle
 operation and it is not production hardened.  
 
We do not want more large dumps of transactions for this run.

It will be much more helpful to do slow transactions over long time to allow height 
and eras to grow over time.  Please limit rate to less than 2/sec as we attempt a long
run and learn from this type of operation.
 
### Pull down genesis files:

[chainspec.toml](https://raw.githubusercontent.com/CasperLabs/casper-node/charlie-testnet/resources/charlie/chainspec.toml)
 - md5: ed4870b20a5e8bf2b4152c6a2d389632  chainspec.toml

[accounts.csv](https://raw.githubusercontent.com/CasperLabs/casper-node/charlie-testnet/resources/charlie/accounts.csv)
 - md5: e094b414dfe5c13f7f98e81a00c82767  accounts.csv

```
sudo curl -o /etc/casper/chainspec.toml https://raw.githubusercontent.com/CasperLabs/casper-node/charlie-testnet/resources/charlie/chainspec.toml
sudo curl -o /etc/casper/accounts.csv https://raw.githubusercontent.com/CasperLabs/casper-node/charlie-testnet/resources/charlie/accounts.csv
```

Pull down to `/etc/casper/`

### Stop Node

`sudo systemctl stop casper-node`

### Clear Local State

`sudo rm -rf /root/.local/share/casper-node`

### Rename Log

`sudo mv /var/log/casper-node.log /var/log/casper-node.log.$(date "+%s")`

### Start Service 

`sudo systemctl start casper-node`

### Log

`/var/log/casper-node.log`

### See status with endpoint:

`http://[NODE IP ADDRESS]:7777/status`
