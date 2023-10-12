# This supports environments where $HOME/.cargo/env has not been sourced (CI, CLion Makefile runner)
CARGO  = $(or $(shell which cargo),  $(HOME)/.cargo/bin/cargo)
RUSTUP = $(or $(shell which rustup), $(HOME)/.cargo/bin/rustup)
NPM    = $(or $(shell which npm),    /usr/bin/npm)

PINNED_NIGHTLY := $(shell cat smart_contracts/rust-toolchain)
PINNED_STABLE  := $(shell sed -nr 's/channel\s+=\s+\"(.*)\"/\1/p' rust-toolchain.toml)
WASM_STRIP_VERSION := $(shell wasm-strip --version)

CARGO_OPTS := --locked
CARGO_PINNED_NIGHTLY := $(CARGO) +$(PINNED_NIGHTLY) $(CARGO_OPTS)
CARGO := $(CARGO) $(CARGO_OPTS)

DISABLE_LOGGING = RUST_LOG=MatchesNothing

# Rust Contracts
ALL_CONTRACTS    = $(shell find ./smart_contracts/contracts/[!.]*  -mindepth 1 -maxdepth 1 -type d -exec basename {} \;)
CLIENT_CONTRACTS = $(shell find ./smart_contracts/contracts/client -mindepth 1 -maxdepth 1 -type d -exec basename {} \;)

# AssemblyScript Contracts
CLIENT_CONTRACTS_AS  = $(shell find ./smart_contracts/contracts_as/client     -mindepth 1 -maxdepth 1 -type d)
TEST_CONTRACTS_AS    = $(shell find ./smart_contracts/contracts_as/test       -mindepth 1 -maxdepth 1 -type d)
EXAMPLE_CONTRACTS_AS    = $(shell find ./smart_contracts/contracts_as/example -mindepth 1 -maxdepth 1 -type d)

CLIENT_CONTRACTS_AS  := $(patsubst %, build-contract-as/%, $(CLIENT_CONTRACTS_AS))
TEST_CONTRACTS_AS    := $(patsubst %, build-contract-as/%, $(TEST_CONTRACTS_AS))
EXAMPLE_CONTRACTS_AS := $(patsubst %, build-contract-as/%, $(EXAMPLE_CONTRACTS_AS))

CONTRACT_TARGET_DIR       = target/wasm32-unknown-unknown/release
CONTRACT_TARGET_DIR_AS    = target_as

build-contract-rs/%:
	cd smart_contracts/contracts && $(CARGO) build --release $(filter-out --release, $(CARGO_FLAGS)) --package $*
	wasm-strip $(CONTRACT_TARGET_DIR)/$(subst -,_,$*).wasm 2>/dev/null | true

.PHONY: build-all-contracts-rs
build-all-contracts-rs:
	cd smart_contracts/contracts && \
	$(CARGO) build --release $(filter-out --release, $(CARGO_FLAGS)) $(patsubst %, -p %, $(ALL_CONTRACTS))

.PHONY: build-client-contracts-rs
build-client-contracts-rs:
	cd smart_contracts/contracts && \
	$(CARGO) build --release $(filter-out --release, $(CARGO_FLAGS)) $(patsubst %, -p %, $(CLIENT_CONTRACTS))

strip-contract/%:
	wasm-strip $(CONTRACT_TARGET_DIR)/$(subst -,_,$*).wasm 2>/dev/null | true

.PHONY: strip-all-contracts
strip-all-contracts: $(info Using 'wasm-strip' version $(WASM_STRIP_VERSION)) $(patsubst %, strip-contract/%, $(ALL_CONTRACTS))

.PHONY: strip-client-contracts
strip-client-contracts: $(patsubst %, strip-contract/%, $(CLIENT_CONTRACTS))

.PHONY: build-contracts-rs
build-contracts-rs: build-all-contracts-rs strip-all-contracts

.PHONY: build-client-contracts
build-client-contracts: build-client-contracts-rs strip-client-contracts

build-contract-as/%:
	cd $* && $(NPM) run asbuild

.PHONY: build-contracts-as
build-contracts-as: \
	$(CLIENT_CONTRACTS_AS) \
	$(TEST_CONTRACTS_AS) \
	$(EXAMPLE_CONTRACTS_AS)

.PHONY: build-contracts
build-contracts: build-contracts-rs build-contracts-as

resources/local/chainspec.toml: generate-chainspec.sh resources/local/chainspec.toml.in
	@./$<

.PHONY: test-rs
test-rs: resources/local/chainspec.toml
	$(LEGACY) $(DISABLE_LOGGING) $(CARGO) test --all-features $(CARGO_FLAGS) -- --nocapture

.PHONY: resources/local/chainspec.toml
test-rs-no-default-features:
	cd smart_contracts/contract && $(DISABLE_LOGGING) $(CARGO) test $(CARGO_FLAGS) --no-default-features --features=version-sync

.PHONY: test-as
test-as: setup-as
	cd smart_contracts/contract_as && npm run asbuild && npm run test

.PHONY: test
test: test-rs-no-default-features test-rs test-as

.PHONY: test-contracts-rs
test-contracts-rs: build-contracts-rs
	$(DISABLE_LOGGING) $(CARGO) test $(CARGO_FLAGS) -p casper-engine-tests -- --ignored

.PHONY: test-contracts-as
test-contracts-as: build-contracts-rs build-contracts-as
	@# see https://github.com/rust-lang/cargo/issues/5015#issuecomment-515544290
	$(DISABLE_LOGGING) $(CARGO) test $(CARGO_FLAGS) --manifest-path "execution_engine_testing/tests/Cargo.toml" --features "use-as-wasm" -- --ignored

.PHONY: test-contracts
test-contracts: test-contracts-rs

.PHONY: check-std-features
check-std-features:
	cd types && $(CARGO) check --all-targets --no-default-features --features=std
	cd types && $(CARGO) check --all-targets --features=std
	cd smart_contracts/contract && $(CARGO) check --all-targets --no-default-features --features=std
	cd smart_contracts/contract && $(CARGO) check --all-targets --features=std

.PHONY: check-format
check-format:
	$(CARGO_PINNED_NIGHTLY) fmt --all -- --check

.PHONY: format
format:
	$(CARGO_PINNED_NIGHTLY) fmt --all

lint-contracts-rs:
	cd smart_contracts/contracts && $(CARGO) clippy $(patsubst %, -p %, $(ALL_CONTRACTS)) -- -D warnings -A renamed_and_removed_lints

.PHONY: lint
lint: lint-contracts-rs lint-default-features lint-all-features lint-smart-contracts

.PHONY: lint-default-features
lint-default-features:
	$(CARGO) clippy --all-targets -- -D warnings

.PHONY: lint-all-features
lint-all-features:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

.PHONY: lint-smart-contracts
lint-smart-contracts:
	cd smart_contracts/contract && $(CARGO) clippy --all-targets -- -D warnings -A renamed_and_removed_lints

.PHONY: audit-rs
audit-rs:
	$(CARGO) audit

.PHONY: audit-as
audit-as:
	@# Runs a vulnerability scan that fails if there are prod vulnerabilities with a moderate level or above.
	cd smart_contracts/contract_as && $(NPM) audit --production --audit-level=moderate

.PHONY: audit
audit: audit-rs

.PHONY: doc
doc:
	RUSTFLAGS="-D warnings" RUSTDOCFLAGS="--cfg docsrs" $(CARGO_PINNED_NIGHTLY) doc --all-features $(CARGO_FLAGS) --no-deps
	cd smart_contracts/contract && RUSTFLAGS="-D warnings" RUSTDOCFLAGS="--cfg docsrs" $(CARGO_PINNED_NIGHTLY) doc --all-features $(CARGO_FLAGS) --no-deps

.PHONY: check-rs
check-rs: \
	check-format \
	doc \
	lint \
	audit \
	check-std-features \
	test-rs \
	test-rs-no-default-features \
	test-contracts-rs

.PHONY: check
check: \
	check-format \
	doc \
	lint \
	audit \
	test \
	test-contracts

.PHONY: clean
clean:
	rm -rf resources/local/chainspec.toml
	rm -rf $(CONTRACT_TARGET_DIR_AS)
	$(CARGO) clean

.PHONY: build-for-packaging
build-for-packaging: build-client-contracts
	$(LEGACY) $(CARGO) build --release

.PHONY: package
package:
	cd contract && $(CARGO) package

.PHONY: publish
publish:
	./publish.sh

.PHONY: bench
bench: build-contracts-rs
	$(CARGO) bench

.PHONY: setup-cargo-packagers
setup-cargo-packagers:
	$(CARGO) install cargo-rpm || exit 0
	$(CARGO) install cargo-deb || exit 0

.PHONY: setup-audit
setup-audit:
	cargo install cargo-audit

.PHONY: setup-rs
setup-rs: smart_contracts/rust-toolchain
	$(RUSTUP) update
	$(RUSTUP) toolchain install $(PINNED_STABLE) $(PINNED_NIGHTLY)
	$(RUSTUP) target add --toolchain $(PINNED_STABLE) wasm32-unknown-unknown
	$(RUSTUP) target add --toolchain $(PINNED_NIGHTLY) wasm32-unknown-unknown

.PHONY: setup-nightly-rs
setup-nightly-rs:
	$(RUSTUP) update
	$(RUSTUP) toolchain install nightly
	$(RUSTUP) target add --toolchain nightly wasm32-unknown-unknown

.PHONY: setup-as
setup-as: smart_contracts/contract_as/package.json
	cd smart_contracts/contract_as && $(NPM) ci

.PHONY: setup
setup: setup-rs
