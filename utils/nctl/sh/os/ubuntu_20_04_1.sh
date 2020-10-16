#!/bin/bash
#

# 0. Update system.
# -----------------------------------
sudo apt update
sudo apt upgrade

# 1. Install pre-requisites.
# -----------------------------------
sudo apt install apt-transport-https autoconf automake ca-certificates curl git libtool lsb-release make openssl.dev g++ pkg-config unzip dirmngr 
sudo snap install cmake --classic
sudo snap install protobuf --classic

# 2. Install rust + tools.
# -----------------------------------
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
export PATH=$HOME/.cargo/bin:$PATH
rustup toolchain install --no-self-update nightly-2020-07-05
rustup target add --toolchain nightly-2020-07-05 wasm32-unknown-unknown

# 3. Install npm + tools.
# -----------------------------------
curl -sL https://deb.nodesource.com/setup_14.x | sudo -E bash -
sudo apt install -y nodejs
sudo npm install -g assemblyscript@0.10.0

# 4. Install python tools (Note: system python=python3).
# -----------------------------------
sudo apt install python3-pip
sudo curl https://pyenv.run | bash
sudo python3 -m pip install pipenv
sudo python3 -m pip install supervisor

# 5. Write paths -> ~.bashrc.
# -----------------------------------
cat >> $HOME/.bashrc <<- EOM

export PATH=\$HOME/.cargo/bin:\$PATH
export PATH=\$HOME/.local/bin:\$PATH
export PATH=\$HOME/.pyenv/bin:\$PATH
eval "$(pyenv init -)"

EOM

# 6. Clone source.
# -----------------------------------
mkdir $HOME/Engineering && cd $HOME/Engineering
git clone https://github.com/CasperLabs/casper-node.git
