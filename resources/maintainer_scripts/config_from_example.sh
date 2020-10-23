#!/usr/bin/env bash
set -e

# This script will generate a config file appropriate to installation machine.

path=/etc/casper/

external_ip=$(dig TXT +short o-o.myaddr.l.google.com @ns1.google.com | tr -d '"')
echo "Using External IP: $external_ip"

config=$path"config.toml"
config_example=$path"config-example.toml"
config_new=$path"config.toml.new"

outfile=$config

if [[ -f $outfile ]]; then
  outfile=$config_new
  if [[ -f $outfile ]]; then
    rm $outfile
  fi
  echo "Previous $config exists, creating as $outfile from $config_example."
  echo "Replace $config with $outfile to use the automatically generated configuration."
else
  echo "Creating $outfile from $config_example."
fi

sed "s/<IP ADDRESS>/${external_ip}/" $config_example > $outfile
