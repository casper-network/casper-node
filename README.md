# casperlabs-node

This is the core application for the CasperLabs blockchain.

## Running a validator node

To run a validator node with the default configuration:

```
cargo run --release -- validator
```

It is very likely that the configuration requires editing though, so typically one will want to generate a configuration file first, edit it and then launch:

```
cargo run --release -- generate-config > mynode.toml
# ... edit mynode.toml
cargo run --release -- validator --config=mynode.toml
```

## Development

A good starting point is to build the documentation and read it in your browser:

```
cargo doc --no-deps --open
```

When generating a configuration file, it is usually helpful to set the log-level to `DEBUG` during development.
