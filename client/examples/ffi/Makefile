# This supports environments where $HOME/.cargo/env has not been sourced (CI, CLion Makefile runner)
CARGO = $(or $(shell which cargo),  $(HOME)/.cargo/bin/cargo)

CASPER_RELEASE_SHARED_LIB = $(abspath ../../../target/release/libcasper_client.so)

CASPER_INCLUDE_PATH = $(abspath ../../headers)

BASEDIR = $(abspath .)

CFLAGS = -Wall -I$(CASPER_INCLUDE_PATH)
LDFLAGS = -lcasper_client -L$(BASEDIR)/build

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

libcasper_client.so: build_dir cargo-build-release
	cp $(CASPER_RELEASE_SHARED_LIB) $(BASEDIR)/build

get_auction_info: libcasper_client.so 
	$(CC) -o build/$@ $(CFLAGS) src/get_auction_info.c $(LDFLAGS)

.PHONY: run_get_auction_info
run_get_auction_info: get_auction_info 
	LD_LIBRARY_PATH=build build/get_auction_info

put_deploy: libcasper_client.so 
	$(CC) -o build/$@ $(CFLAGS) src/put_deploy.c $(LDFLAGS)

.PHONY: run_put_deploy
run_put_deploy: put_deploy 
	LD_LIBRARY_PATH=build build/put_deploy

.PHONY: run-examples
run-examples: run_get_auction_info run_put_deploy
