# CasperLabs node

The is the core application for the CasperLabs blockchain.

## Building

To compile this application, simply run `cargo build` on a recent stable Rust (`>= 1.43.1`) version.

## Running a validator node

Launching a validator node with the default configuration is done by simply launching the application:

```
casper-node validator
```

It is very likely that the configuration requires editing though, so typically one will want to generate a configuration file first, edit it and then launch:

```
casper-node generate-config > mynode.toml
# ... edit mynode.toml
casper-node validator -c mynode.toml
```

## Development

A good starting point is to build the documentation and read it in your browser:

```
cargo doc --no-deps --open
```