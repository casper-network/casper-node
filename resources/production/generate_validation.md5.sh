#!/usr/bin/env bash

rm validation.md5 || true
md5sum chainspec.toml >> validation.md5
md5sum accounts.csv >> validation.md5
md5sum -c validation.md5
