prepare:
	rustup target add wasm32-unknown-unknown

build-erc20:
	cd erc20_token && cargo build --release --target wasm32-unknown-unknown
	wasm-strip erc20_token/target/wasm32-unknown-unknown/release/erc20_token.wasm 2>/dev/null | true

test: build-erc20
	mkdir -p tests/wasm
	cp erc20_token/target/wasm32-unknown-unknown/release/erc20_token.wasm tests/wasm
	cd tests && cargo test

clippy:
	cd erc20_token && cargo clippy --all-targets -- -D warnings
	cd tests && cargo clippy --all-targets -- -D warnings

check-lint: clippy
	cd erc20_token && cargo fmt -- --check
	cd tests && cargo fmt -- --check

lint: clippy
	cd erc20_token && cargo fmt
	cd tests && cargo fmt

clean:
	cd erc20_token && cargo clean
	cd tests && cargo clean
	rm -rf tests/wasm
