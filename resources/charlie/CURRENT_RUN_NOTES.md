# Charlie-5 Run Notes

## Start Time: 2020-09-24 17:00 UTC

## Changes

 - Network name updated to `casper-charlie-testnet-6`.
 - Fix outdated confirmation warnings.
 - Log in more detail if not voting in an era.
 - Added log messages when not sending confirmation.
 - Caching chainspec.toml for helping deal with chainspec db access.
 - Removed panic of outdated block is exectuted.
 - impl Display for validators to clean up logs.
 
## Testing

We have learned what we will from load testing this as we did not have expectations for more than mostly idle
 operation and it is not production hardened.  
 
We do not want more large dumps of transactions for this run.

It will be much more helpful to do slow transactions over long time to allow height 
and eras to grow over time.  Please limit rate to less than 2/sec as we attempt a long
run and learn from this type of operation.
 
### Pull down genesis files:

[chainspec.toml](https://raw.githubusercontent.com/CasperLabs/casper-node/charlie-testnet/resources/charlie/chainspec.toml)
 - md5: 0b5fa5e7b22ae186c72f8cc1a5825548  chainspec.toml

[accounts.csv](https://raw.githubusercontent.com/CasperLabs/casper-node/charlie-testnet/resources/charlie/accounts.csv)
 - md5: e511b7407731ccbed243cec2ad5221fa  accounts.csv

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
