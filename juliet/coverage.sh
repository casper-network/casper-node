#!/bin/sh
# coverage.sh: Runs a coverage utility
#
# Requires cargo-tarpaulin and lcov to be installed.
# You can install ryanluker.vscode-coverage-gutters in VSCode to visualize missing coverage.

set -e

# Try to make sure there is reasonable coverage on fuzzed tests.
export PROPTEST_CASES=10000

cargo tarpaulin --engine Llvm -r . --exclude-files '../**' --exclude-files 'examples' --out lcov
mkdir -p coverage
genhtml -o coverage lcov.info
