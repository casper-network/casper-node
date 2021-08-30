validation
===============

Automation tool to validate the code based on fixtures.

What is casper-validation?
--------------------------------------

This tool validates the code by loading fixtures JSON that contains an input, and the expected output by applying an operation.

Usage
--------------------------------------

To generate new fixtures with a generator run:

```
$ cargo run -p casper-validation -- generate --output utils/validation/tests/fixtures
```

**Important note**

Do not use this with day to day development - for example to fix an error in serialization code by replacing the fixture with possibly invalid code.

To validate the implementation using all the fixtures:

```
$ cargo test -p casper-validation
```
