# nctl setup

### Step 0 - pre-requisites.

1. python3 + pip3.
2. The casper-node software (https://github.com/CasperLabs/casper-node) cloned into YOUR_WORKING_DIRECTORY.

### Step 1 - install pre-requisites.

```
# Supervisor - cross-platform process manager.
python3 -m pip install supervisor

# Rust toolchain - required by casper-node software.
cd YOUR_WORKING_DIRECTORY/casper-node
make setup-rs
```

### Step 2 - set local setup file.

```
cd YOUR_WORKING_DIRECTORY/casper-node

cat >> $HOME/.casper-nctl <<- EOM

# Activate nctl shell.
. $(pwd)/utils/nctl/activate

EOM
```

### Step 3 - extend bashrc file to make nctl commands available from terminal session.

```
cat >> $HOME/.bashrc <<- EOM

# ----------------------------------------------------------------------
# CASPER - NCTL
# ----------------------------------------------------------------------

# Activate nctl shell.
. $HOME/.casper-nctl

EOM
```

### Step 4 - activate nctl shell.

```
. $HOME/.casper-nctl
```
