PREFIX ?= ~/.local/bin
BIN := storysquish

rust-version:
	rustc --version 		# rust compiler
	cargo --version 		# cargo package manager
	rustfmt --version 		# rust formatter
	rustup --version 		# rust toolchain manager
	clippy-driver --version	# rust linter

format:
	cargo fmt --all

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-features

check:
	cargo fmt --all --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-features

run:
	cargo run -r

build:
	cargo update
	cargo build

all:
	cargo build -r --bin $(BIN)
	strip target/release/$(BIN)

install: all
	cp target/release/$(BIN) $(PREFIX)

clean:
	cargo clean

uninstall: clean
	rm -f $(PREFIX)/$(BIN)

upgrade:
	rustup update
	cargo update

.PHONY: rust-version format lint test check run build all install clean uninstall upgrade
