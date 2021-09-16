# Logging

## General

The `logging` module provides the ability to log messages from any Casper crate to `stdout` using the canonical
macros from the [`log` crate](https://crates.io/crates/log).

It also provides functions to allow logging messages with properties attached for the purpose of structured logging and
integration with tools like [Prometheus](https://prometheus.io/).

Logging can be initialized to support outputting metrics, regardless of the chosen log-level, and can also be set to
display messages in a human-readable format or a hybrid structured one, with each line containing a human-readable
component followed by JSON formatted details.

## Usage

#### In libraries

Libraries should link only to the `log` crate, and use the provided macros to log whatever information will be useful to
downstream consumers.

#### In executables

Logging can be initialized using the `initialize()` function, and should be done early in the runtime of
the program.

#### In tests

Logging can also be initialized early in a test's execution.  Note that constructing a
[`TestContextBuilder`][TestContextBuilder] will automatically enable logging at warn-level in the human-readable
format.  To avoid this, call `initialize()` with required settings before constructing the first
`TestContextBuilder`.

Bear in mind that by default tests are run in parallel on multiple threads, so initializing logging might need to be
done in several or all of the tests in a single binary.

## Metrics

The structured log messages output via `log_metric()` or `log_duration()` can be
parsed and read by the `casper-engine-metrics-scraper`.

This tool reads from `stdin`, extracts the "time-series-data" from the log messages' properties and makes the values
available via a `GET` endpoint.


[TestContextBuilder]: https://docs.rs/casper-engine-test-support/latest/casper_engine_test_support/struct.TestContextBuilder.html
