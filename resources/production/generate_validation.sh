#!/usr/bin/env bash

md5sum accounts.csv > validation.md5
md5sum chainspec.toml >> validation.md5
