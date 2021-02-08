#!/bin/sh

kubectl -n casper-mynet port-forward casper-node-2 7777 &
kubectl -n casper-mynet port-forward casper-node-3 7778:7777 &
wait
