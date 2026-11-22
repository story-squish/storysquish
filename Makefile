BIN := storysquish
APP_NAME := StorySquish
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
OS := $(shell uname -s)

ifeq ($(OS),Darwin)
APP_DIR ?= /Applications
APP_BUNDLE := $(APP_DIR)/$(APP_NAME).app
else
PREFIX ?= $(HOME)/.local
BIN_DIR := $(PREFIX)/bin
DESKTOP_FILE := $(PREFIX)/share/applications/$(BIN).desktop
endif

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

ifeq ($(OS),Darwin)
install: all
	rm -rf "$(APP_BUNDLE)"
	mkdir -p "$(APP_BUNDLE)/Contents/MacOS"
	cp target/release/$(BIN) "$(APP_BUNDLE)/Contents/MacOS/$(BIN)"
	printf '%s\n' \
		'<?xml version="1.0" encoding="UTF-8"?>' \
		'<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
		'<plist version="1.0"><dict>' \
		'<key>CFBundleName</key><string>$(APP_NAME)</string>' \
		'<key>CFBundleDisplayName</key><string>$(APP_NAME)</string>' \
		'<key>CFBundleIdentifier</key><string>io.github.$(BIN)</string>' \
		'<key>CFBundleExecutable</key><string>$(BIN)</string>' \
		'<key>CFBundlePackageType</key><string>APPL</string>' \
		'<key>CFBundleVersion</key><string>$(VERSION)</string>' \
		'<key>CFBundleShortVersionString</key><string>$(VERSION)</string>' \
		'<key>NSHighResolutionCapable</key><true/>' \
		'</dict></plist>' > "$(APP_BUNDLE)/Contents/Info.plist"
	codesign --force --sign - "$(APP_BUNDLE)"

uninstall: clean
	rm -rf "$(APP_BUNDLE)"
else
install: all
	install -Dm755 target/release/$(BIN) "$(BIN_DIR)/$(BIN)"
	mkdir -p "$(dir $(DESKTOP_FILE))"
	printf '%s\n' \
		'[Desktop Entry]' \
		'Type=Application' \
		'Name=$(APP_NAME)' \
		'Comment=Scene board for planning and drafting novels' \
		'Exec=$(BIN_DIR)/$(BIN) %f' \
		'Terminal=false' \
		'Categories=Office;' \
		'StartupWMClass=$(BIN)' > "$(DESKTOP_FILE)"

uninstall: clean
	rm -f "$(BIN_DIR)/$(BIN)" "$(DESKTOP_FILE)"
endif

clean:
	cargo clean

upgrade:
	rustup update
	cargo update

.PHONY: rust-version format lint test check run build all install clean uninstall upgrade
