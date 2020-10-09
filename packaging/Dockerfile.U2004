# Container for debian packaging CasperLabs
# focal = 20.04
FROM ubuntu:focal

# DEBIAN_FRONTEND required for tzdata dependency install
RUN apt-get update \
    && DEBIAN_FRONTEND="noninteractive" \
        apt-get install -y curl gnupg gcc git ca-certificates \
              protobuf-compiler libprotobuf-dev \
              pkg-config libssl-dev make build-essential

# install cmake
RUN curl -Ls https://github.com/Kitware/CMake/releases/download/v3.17.3/cmake-3.17.3-Linux-x86_64.tar.gz | tar -C /usr/local --strip-components=1 -xz

# Install node.js, npm, and assemblyscript
RUN curl -sL https://deb.nodesource.com/setup_12.x | bash - \
    && apt-get -y install nodejs \
    && npm install --global remark-cli remark-validate-links remark-lint-no-dead-urls "assemblyscript@0.9.1"

RUN curl -f -L https://static.rust-lang.org/rustup.sh -O \
    && sh rustup.sh -y
ENV PATH="$PATH:/root/.cargo/bin"

RUN echo export RUST_TOOLCHAIN=$(curl -s https://raw.githubusercontent.com/CasperLabs/CasperLabs/dev/execution-engine/rust-toolchain) >> ~/.rust_env \
    && . ~/.rust_env; rustup toolchain install "${RUST_TOOLCHAIN}" \
    && . ~/.rust_env; rustup toolchain install stable \
    && rustup update \
    && . ~/.rust_env; cargo +${RUST_TOOLCHAIN} install cargo-deb

RUN git clone https://github.com/CasperLabs/casper-node.git ~/casper-node \
    && cd ~/casper-node \
    && make setup \
    && ~/.cargo/bin/cargo fetch \
    && rm -rf ~/casper-node
