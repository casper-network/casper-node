# This supports environments where $HOME/.cargo/env has not been sourced (CI, CLion Makefile runner)
CARGO = $(or $(shell which cargo),  $(HOME)/.cargo/bin/cargo)

# TODO: The resulting binaries from this example will have absolute paths embedded, which might not be desirable.
# TODO: one option is copying libcasper_client.so to PWD and then linking directly.
CASPER_SHARED_LIB = $(abspath ../../../target/release/libcasper_client.so)

CASPER_HEADER_PATH = $(abspath ../../headers)

BASEDIR = $(abspath .)

CFLAGS = -Wall -I$(CASPER_HEADER_PATH)
LDFLAGS = -Wl,$(CASPER_SHARED_LIB)

.PHONY: all
all: check-format get_auction_info put_deploy

.PHONY: cargo-build-release
cargo-build-release:
	$(CARGO) build --release

CLANG_FORMAT = $(shell which clang-format)
.PHONY: check-format
check-format:
	@bash check_format.bash

.PHONY: format
format:
	$(CLANG_FORMAT) -i --verbose src/*.c

.PHONY: build_dir
build_dir:
	@mkdir -p build

.PHONY: clean
clean:
	@rm -rf build

get_auction_info: build_dir cargo-build-release
	$(CC) -o build/$@ $(CFLAGS) src/get_auction_info.c $(LDFLAGS)

put_deploy: build_dir cargo-build-release
	$(CC) -o build/$@ $(CFLAGS) src/put_deploy.c $(LDFLAGS)