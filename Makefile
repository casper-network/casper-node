# This supports environments where $HOME/.cargo/env has not been sourced (CI, CLion Makefile runner)
CARGO  = $(or $(shell which cargo),  $(HOME)/.cargo/bin/cargo)
RUSTUP = $(or $(shell which rustup), $(HOME)/.cargo/bin/rustup)
CARGO_AUDIT = $(or $(shell which cargo-audit), $(HOME)/.cargo/bin/cargo-audit)

PINNED_NIGHTLY := $(shell cat smart_contracts/rust-toolchain)
PINNED_STABLE  := $(shell sed -nr 's/channel *= *\"(.*)\"/\1/p' rust-toolchain.toml)
CARGO_AUDIT_VERSION := 0.22.1
WASM_STRIP_VERSION := $(shell wasm-strip --version)

CARGO_OPTS := --locked
CARGO_PINNED_NIGHTLY := $(CARGO) +$(PINNED_NIGHTLY) $(CARGO_OPTS)
CARGO := $(CARGO) $(CARGO_OPTS)

DISABLE_LOGGING = RUST_LOG=MatchesNothing

# Rust Contracts
VM2_CONTRACTS    = $(shell find ./smart_contracts/contracts/vm2 -mindepth 1 -maxdepth 1 -type d -exec basename {} \;)
ALL_CONTRACTS    = $(shell find ./smart_contracts/contracts/[!.]* -mindepth 1 -maxdepth 1 -not -path "./smart_contracts/contracts/vm2*" -type d -exec basename {} \;)
CLIENT_CONTRACTS = $(shell find ./smart_contracts/contracts/client -mindepth 1 -maxdepth 1 -type d -exec basename {} \;)
EVM_CONTRACTS    = $(shell find ./smart_contracts/evm_contracts -mindepth 1 -maxdepth 1 -name '*.sol' -exec basename {} .sol \;)
CARGO_HOME_REMAP = $(if $(CARGO_HOME),$(CARGO_HOME),$(HOME)/.cargo)
RUSTC_FLAGS      = "--remap-path-prefix=$(CARGO_HOME_REMAP)=/home/cargo --remap-path-prefix=$$PWD=/dir"
WASM_RUSTC_FLAGS = "--remap-path-prefix=$(CARGO_HOME_REMAP)=/home/cargo --remap-path-prefix=$$PWD=/dir -C target-cpu=mvp -C target-feature=-bulk-memory"
CARGO_TEST_PROFILE_ENV ?=

CONTRACT_TARGET_DIR       = target/wasm32-unknown-unknown/release
EVM_CONTRACT_TARGET_DIR   = target/evm-contracts

build-contract-rs/%:
	cd smart_contracts/contracts && RUSTFLAGS=$(WASM_RUSTC_FLAGS) $(CARGO) build --verbose --release -Z build-std=std,core,alloc,panic_abort $(filter-out --release, $(CARGO_FLAGS)) --package $*

build-vm2-contract-rs/%:
	CMAKE_POLICY_VERSION_MINIMUM=3.5 $(CARGO) build -p cargo-casper --bin cargo-casper
	RUSTFLAGS=$(RUSTC_FLAGS) $(CURDIR)/target/debug/cargo-casper build-schema --package $*
	cd smart_contracts/contracts/vm2 && RUSTFLAGS=$(WASM_RUSTC_FLAGS) $(CARGO) build --verbose --release -Z build-std=std,core,alloc,panic_abort $(filter-out --release, $(CARGO_FLAGS)) --package $*

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

.PHONY: setup-evm
setup-evm:
	@command -v solc >/dev/null || (echo "solc is required to build EVM contract fixtures" && exit 1)

build-contract-evm/%: setup-evm
	mkdir -p $(EVM_CONTRACT_TARGET_DIR)
	solc --optimize --abi --bin --overwrite -o $(EVM_CONTRACT_TARGET_DIR) smart_contracts/evm_contracts/$*.sol

.PHONY: build-contracts-evm
build-contracts-evm: $(patsubst %, build-contract-evm/%, $(EVM_CONTRACTS))

.PHONY: test-contracts-evm
test-contracts-evm: build-contracts-evm
	$(DISABLE_LOGGING) $(CARGO_TEST_PROFILE_ENV) $(CARGO) test --all-features $(CARGO_FLAGS) -p casper-executor-evm

resources/local/chainspec.toml: generate-chainspec.sh resources/local/chainspec.toml.in
	@./$<

.PHONY: build-test-artifacts
build-test-artifacts: resources/local/chainspec.toml build-contracts-rs

.PHONY: test-rs
test-rs:
	$(LEGACY) $(DISABLE_LOGGING) $(CARGO_TEST_PROFILE_ENV) $(CARGO) test --all-features --no-fail-fast $(CARGO_FLAGS) -- --nocapture

.PHONY: test-rs-ci
test-rs-ci:
	$(LEGACY) $(DISABLE_LOGGING) $(CARGO_TEST_PROFILE_ENV) $(CARGO) test --all-features --no-fail-fast $(CARGO_FLAGS) -- --nocapture --skip reactor::main_reactor::tests::rewards
	$(LEGACY) $(DISABLE_LOGGING) $(CARGO_TEST_PROFILE_ENV) $(CARGO) test --all-features --no-fail-fast $(CARGO_FLAGS) -- --nocapture --test-threads=1 reactor::main_reactor::tests::rewards

.PHONY: resources/local/chainspec.toml
test-rs-no-default-features:
	cd smart_contracts/contract && $(DISABLE_LOGGING) $(CARGO_TEST_PROFILE_ENV) $(CARGO) test $(CARGO_FLAGS) --no-default-features --features=version-sync

.PHONY: test
test: build-test-artifacts test-rs-no-default-features test-rs

.PHONY: test-ci
test-ci: test-rs-no-default-features test-rs-ci

.PHONY: test-contracts-rs
test-contracts-rs:
	$(DISABLE_LOGGING) $(CARGO_TEST_PROFILE_ENV) $(CARGO) test $(CARGO_FLAGS) -p casper-engine-tests -- --ignored --skip repeated_ffi_call_should_gas_out_quickly

.PHONY: test-contracts-timings
test-contracts-timings: resources/local/chainspec.toml build-contracts-rs
	$(DISABLE_LOGGING) $(CARGO) test --release $(filter-out --release, $(CARGO_FLAGS)) -p casper-engine-tests -- --ignored --test-threads=1 repeated_ffi_call_should_gas_out_quickly

.PHONY: test-contracts
test-contracts: build-test-artifacts test-contracts-rs

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
	cd smart_contracts/contracts && $(CARGO) clippy $(patsubst %, -p %, $(ALL_CONTRACTS)) -- -A renamed_and_removed_lints

.PHONY: lint
lint: lint-contracts-rs lint-default-features lint-all-features lint-smart-contracts lint-no-default-features

.PHONY: lint-default-features
lint-default-features:
	$(CARGO) clippy --all-targets

.PHONY: lint-no-default-features
lint-no-default-features:
	$(CARGO) clippy --all-targets --no-default-features

.PHONY: lint-all-features
lint-all-features:
	LC_ALL=C LANG=C LC_CTYPE=C $(CARGO) clippy --all-targets --all-features

.PHONY: lint-smart-contracts
lint-smart-contracts:
	cd smart_contracts/contract && $(CARGO) clippy --all-targets -- -A renamed_and_removed_lints

.PHONY: audit-rs
audit-rs:
	$(CARGO) audit \
		--ignore RUSTSEC-2025-0055 \
		--ignore RUSTSEC-2026-0194 \
		--ignore RUSTSEC-2026-0195

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
	$(RUSTUP) component add rust-src --toolchain $(PINNED_NIGHTLY)
	$(CARGO_AUDIT) --version 2>/dev/null | grep -q ' $(CARGO_AUDIT_VERSION)$$' || \
		$(CARGO) install cargo-audit --version '=$(CARGO_AUDIT_VERSION)'

.PHONY: setup
setup: setup-rs
