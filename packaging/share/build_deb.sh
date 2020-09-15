#!/usr/bin/env bash

cd ..
git clone https://github.com/CasperLabs/casper-node.git ./casper-node
cd casper-node

make deb

cp ./target/debian/* /root/share/
