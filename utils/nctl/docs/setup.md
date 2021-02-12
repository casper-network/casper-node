# NCTL setup

### Step 0 - pre-requisites.

0. bash shell.
1. python3 + pip3.
2. The casper-node software (https://github.com/CasperLabs/casper-node) cloned into YOUR_WORKING_DIRECTORY.
3. The casper-node-launcher software (https://github.com/CasperLabs/casper-node-launcher) cloned into YOUR_WORKING_DIRECTORY.

### Step 1 - install pre-requisites.

```
# Supervisor - cross-platform process manager.
python3 -m pip install supervisor

# toml - Config file parser.
python3 -m pip install toml

# Rust toolchain and smart contracts - required by casper-node software.
cd YOUR_WORKING_DIRECTORY/casper-node
make setup-rs
```

### Step 2 - extend bashrc file to make NCTL commands available from terminal session.

```
cd YOUR_WORKING_DIRECTORY/casper-node

cat >> $HOME/.bashrc <<- EOM

# ----------------------------------------------------------------------
# CASPER - NCTL
# ----------------------------------------------------------------------

# Activate NCTL shell.
. $(pwd)/utils/nctl/activate

EOM
```

### Step 3 - refresh bash session.

```
. $HOME/.bashrc
```
