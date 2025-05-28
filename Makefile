# This supports environments where $HOME/.cargo/env has not been sourced (CI, CLion Makefile runner)
CARGO  = $(or $(shell which cargo),  $(HOME)/.cargo/bin/cargo)
RUSTUP = $(or $(shell which rustup), $(HOME)/.cargo/bin/rustup)

PINNED_NIGHTLY := $(shell cat smart_contracts/rust-toolchain)
PINNED_STABLE  := $(shell sed -nr 's/channel *= *\"(.*)\"/\1/p' rust-toolchain.toml)
WASM_STRIP_VERSION := $(shell wasm-strip --version)

CARGO_OPTS := --locked
CARGO_PINNED_NIGHTLY := $(CARGO) +$(PINNED_NIGHTLY) $(CARGO_OPTS)
CARGO := $(CARGO) $(CARGO_OPTS)

DISABLE_LOGGING = RUST_LOG=MatchesNothing

# Rust Contracts
VM2_CONTRACTS    = $(shell find ./smart_contracts/contracts/vm2 -mindepth 1 -maxdepth 1 -type d -exec basename {} \;)
ALL_CONTRACTS    = $(shell find ./smart_contracts/contracts/[!.]* -mindepth 1 -maxdepth 1 -not -path "./smart_contracts/contracts/vm2*" -type d -exec basename {} \;)
CLIENT_CONTRACTS = $(shell find ./smart_contracts/contracts/client -mindepth 1 -maxdepth 1 -type d -exec basename {} \;)
CARGO_HOME_REMAP = $(if $(CARGO_HOME),$(CARGO_HOME),$(HOME)/.cargo)
RUSTC_FLAGS      = "--remap-path-prefix=$(CARGO_HOME_REMAP)=/home/cargo --remap-path-prefix=$$PWD=/dir"

CONTRACT_TARGET_DIR       = target/wasm32-unknown-unknown/release

build-contract-rs/%:
	cd smart_contracts/contracts && RUSTFLAGS=$(RUSTC_FLAGS) $(CARGO) build --verbose --release $(filter-out --release, $(CARGO_FLAGS)) --package $*

build-vm2-contract-rs/%:
	RUSTFLAGS=$(RUSTC_FLAGS) $(CARGO) run -p cargo-casper --bin cargo-casper -- build-schema --package $*
	cd smart_contracts/contracts/vm2 && RUSTFLAGS=$(RUSTC_FLAGS) $(CARGO) build --verbose --release $(filter-out --release, $(CARGO_FLAGS)) --package $*

.PHONY: build-vm2-contracts-rs
build-vm2-contracts-rs: $(patsubst %, build-vm2-contract-rs/%, $(VM2_CONTRACTS))

.PHONY: build-all-contracts-rs
build-all-contracts-rs: $(patsubst %, build-contract-rs/%, $(ALL_CONTRACTS))

.PHONY: build-client-contracts-rs
build-client-contracts-rs: $(patsubst %, build-contract-rs/%, $(CLIENT_CONTRACTS))

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

.PHONY: build-contracts
build-contracts: build-contracts-rs

resources/local/chainspec.toml: generate-chainspec.sh resources/local/chainspec.toml.in
	@./$<

.PHONY: test-rs
test-rs: resources/local/chainspec.toml build-contracts-rs
	$(LEGACY) $(DISABLE_LOGGING) $(CARGO) test --all-features --no-fail-fast $(CARGO_FLAGS) -- --nocapture

.PHONY: resources/local/chainspec.toml
test-rs-no-default-features:
	cd smart_contracts/contract && $(DISABLE_LOGGING) $(CARGO) test $(CARGO_FLAGS) --no-default-features --features=version-sync

.PHONY: test
test: test-rs-no-default-features test-rs

.PHONY: test-contracts-rs
test-contracts-rs: build-contracts-rs
	$(DISABLE_LOGGING) $(CARGO) test $(CARGO_FLAGS) -p casper-engine-tests -- --ignored --skip repeated_ffi_call_should_gas_out_quickly

.PHONY: test-contracts-timings
test-contracts-timings: build-contracts-rs
	$(DISABLE_LOGGING) $(CARGO) test --release $(filter-out --release, $(CARGO_FLAGS)) -p casper-engine-tests -- --ignored --test-threads=1 repeated_ffi_call_should_gas_out_quickly

.PHONY: test-contracts
test-contracts: test-contracts-rs

.PHONY: check-no-default-features
check-no-default-features:
	cd types && $(CARGO) check --all-targets --no-default-features

.PHONY: check-std-features
check-std-features:
	cd types && $(CARGO) check --all-targets --no-default-features --features=std
	cd types && $(CARGO) check --all-targets --features=std
	cd smart_contracts/contract && $(CARGO) check --all-targets --no-default-features --features=std
	cd smart_contracts/contract && $(CARGO) check --all-targets --features=std

check-std-fs-io-features:
	cd types && $(CARGO) check --all-targets --features=std-fs-io
	cd types && $(CARGO) check --lib --features=std-fs-io

check-testing-features:
	cd types && $(CARGO) check --all-targets --no-default-features --features=testing
	cd types && $(CARGO) check --all-targets --features=testing

.PHONY: check-format
check-format:
	$(CARGO_PINNED_NIGHTLY) fmt --all -- --check

.PHONY: format
format:
	$(CARGO_PINNED_NIGHTLY) fmt --all

lint-contracts-rs:
	cd smart_contracts/contracts && $(CARGO) clippy $(patsubst %, -p %, $(ALL_CONTRACTS)) -- -D warnings -A renamed_and_removed_lints

.PHONY: lint
lint: lint-contracts-rs lint-default-features lint-all-features lint-smart-contracts lint-no-default-features

.PHONY: lint-default-features
lint-default-features:
	$(CARGO) clippy --all-targets -- -D warnings

.PHONY: lint-no-default-features
lint-no-default-features:
	$(CARGO) clippy --all-targets --no-default-features -- -D warnings

.PHONY: lint-all-features
lint-all-features:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

.PHONY: lint-smart-contracts
lint-smart-contracts:
	cd smart_contracts/contract && $(CARGO) clippy --all-targets -- -D warnings -A renamed_and_removed_lints

.PHONY: audit-rs
audit-rs:
	$(CARGO) audit --ignore RUSTSEC-2024-0437 --ignore RUSTSEC-2025-0022

.PHONY: audit
audit: audit-rs

.PHONY: doc
doc:
	RUSTFLAGS="-D warnings" RUSTDOCFLAGS="--cfg docsrs" $(CARGO_PINNED_NIGHTLY) doc --all-features $(CARGO_FLAGS) --no-deps
	cd smart_contracts/contract && RUSTFLAGS="-D warnings" RUSTDOCFLAGS="--cfg docsrs" $(CARGO_PINNED_NIGHTLY) doc --all-features $(CARGO_FLAGS) --no-deps

.PHONY: check-rs
check: \
	check-format \
	doc \
	lint \
	audit \
	check-no-default-features \
	check-std-features \
	check-std-fs-io-features \
	check-testing-features \
	test-rs \
	test-rs-no-default-features \
	test-contracts-rs

.PHONY: clean
clean:
	rm -rf resources/local/chainspec.toml
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
	$(CARGO) install cargo-deb || exit 0

.PHONY: setup-rs
setup-rs:
	$(RUSTUP) update
	$(RUSTUP) toolchain install $(PINNED_STABLE) $(PINNED_NIGHTLY)
	$(RUSTUP) target add --toolchain $(PINNED_STABLE) wasm32-unknown-unknown
	$(RUSTUP) target add --toolchain $(PINNED_NIGHTLY) wasm32-unknown-unknown
	$(RUSTUP) component add --toolchain $(PINNED_NIGHTLY) rustfmt clippy-preview
	$(RUSTUP) component add --toolchain $(PINNED_STABLE) clippy-preview
	$(CARGO) install cargo-audit

.PHONY: setup
setup: setup-rs
