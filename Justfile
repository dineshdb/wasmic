
# Installation (for development)
install:
	#!/bin/bash
	cargo install --path . -f
	if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then mkdir -p "$APPDATA/wasic/" && cp config.yaml "$APPDATA/wasic/"; \
	elif [[ "$OSTYPE" == "darwin"* ]]; then mkdir -p "$HOME/Library/Application Support/wasic/" && cp config.yaml "$HOME/Library/Application Support/wasic/"; \
	elif [[ "$OSTYPE" == "linux-gnu"* ]]; then mkdir -p ~/.config/wasic/ && cp config.yaml ~/.config/wasic/; \
	fi

# Development commands
fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy -- -D warnings
	cargo test
	cargo machete

lint-fix: fmt
	cargo clippy --fix --allow-dirty --allow-staged
	cargo sort
	cargo machete --fix

# CI tool installation (using cargo-binstall, minimal dependencies)
install-tools:
	#!/bin/bash
	@echo "Installing cargo-binstall..."
	if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then \
		where cargo-binstall > nul 2>&1 || powershell -Command "irm https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | iex"; \
	else \
		which cargo-binstall > /dev/null || curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash; \
	fi
	@echo "Installing rustfmt..."
	cargo fmt --version > /dev/null || rustup component add rustfmt
	cargo clippy --version > /dev/null || rustup component add clippy
	cargo binstall cargo-sort
	cargo binstall cargo-machete
	
# Application commands
call: call-time call-fetch
call-time:
	cargo run -- call --config config.yaml --function "time.get-current-time" --args "{}"

call-fetch:
	cargo run -- call --config config.yaml --function "fetch.fetch" --args '{"url":"https://httpbin.org/get"}'

list:
	cargo run -- --config config.yaml list

# Full test suite
full-test:
	cargo test
	cargo clippy -- -D warnings
	@echo "✅ Full test suite passed"

# Setup for new developers
setup: install-tools
	@echo "✅ Development environment setup complete"

search:
	cargo run -- call --config config.yaml --function "brave_search.search" --args '{"params": {"query": "AI news artificial intelligence latest developments","limit": 10,"country": "US","language": "en","safe-search": "moderate","include-text": true}}'
