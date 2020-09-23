# Charlie-4 Run Notes

## Start Time: 2020-09-23 17:00 UTC

### Pull down genesis files:

[chainspec.toml](https://raw.githubusercontent.com/CasperLabs/casper-node/537e0fb921f9f620783dac158334782b1fffff62/resources/charlie/chainspec.toml)
 - md5: 0bae704c9262072926033bbc4813e47e  chainspec.toml

[accounts.csv](https://raw.githubusercontent.com/CasperLabs/casper-node/charlie-testnet/resources/charlie/accounts.csv)
 - md5: e094b414dfe5c13f7f98e81a00c82767  accounts.csv

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

http://[NODE IP ADDRESS]:7777/status
