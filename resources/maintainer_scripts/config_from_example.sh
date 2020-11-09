#!/usr/bin/env bash
set -e

# This script will generate a CONFIG file appropriate to installation machine.

CONFIG_PATH=/etc/casper/
CONFIG=$CONFIG_PATH"config.toml"
CONFIG_EXAMPLE=$CONFIG_PATH"config-example.toml"
CONFIG_NEW=$CONFIG_PATH"config.toml.new"

IPv4_STRING='(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)'

re='^(0*(1?[0-9]{1,2}|2([0-4][0-9]|5[0-5]))\.){3}'

function curl_ext_ip()
{
  result=$(curl -s --max-time 10 --connect-timeout 10 "$1") || result='dead pipe'
}

URLS=("https://checkip.amazonaws.com" "https://ifconfig.me" "https://ident.me")
NAMES=("amazonaws.com" "ifconfig.me" "ident.me")
RESULTS=()
array_len=${#URLS[@]}

echo && echo -e "Trying to get external IP from couple of services ..."

for (( i=0; i<$array_len; i++ )); do
  curl_ext_ip "${URLS[$i]}"
  if [[ $result != "dead pipe" ]]; then
    RESULTS+=($result)
  fi
  echo -e "${NAMES[$i]} report: $result"
done

EXTERNAL_IP=$(echo "${RESULTS[@]}" | awk '{for(i=1;i<=NF;i++) print $i}' | awk '!x[$0]++' | grep -E -o "$IPv4_STRING" | head -n 1)

if ! [[ $EXTERNAL_IP =~ $re ]]; then
 echo -e
 echo -e "WARNING: Can't get external VPS IP automatically."
 echo -e "Please manually create $CONFIG from $CONFIG_EXAMPLE"
 echo -e "by replacing <IP_ADDRESS> with your external IP after installation."
 echo
 sleep 2
 exit 0
else
 echo && echo -e "Using External IP: $EXTERNAL_IP" && echo
fi

OUTFILE=$CONFIG

if [[ -f $OUTFILE ]]; then
 OUTFILE=$CONFIG_NEW
 if [[ -f $OUTFILE ]]; then
   rm $OUTFILE
 fi
 echo "Previous $CONFIG exists, creating as $OUTFILE from $CONFIG_EXAMPLE."
 echo "Replace $CONFIG with $OUTFILE to use the automatically generated configuration."
else
 echo "Creating $OUTFILE from $CONFIG_EXAMPLE."
fi

sed "s/<IP ADDRESS>/${EXTERNAL_IP}/" $CONFIG_EXAMPLE > $OUTFILE