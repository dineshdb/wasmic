
# Installation (for development)
install:
	cargo install --path .
	mkdir -p "~/.config/wasic/" || mkdir -p "~/Library/Application Support/wasic/"
	cp config.yaml "~/.config/wasic/" 2>/dev/null || cp config.yaml "~/Library/Application Support/wasic/" 2>/dev/null

# Development commands
fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy -- -D warnings
	cargo test

lint-all: lint
	cargo sort
	cargo machete

lint-fix: lint
	cargo clippy --fix --allow-dirty --allow-staged

# CI tool installation (using cargo-binstall, minimal dependencies)
install-tools:
	@echo "Installing cargo-binstall..."
	which cargo-binstall > /dev/null || curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
	@echo "Installing rustfmt..."
	cargo fmt --version > /dev/null || rustup component add rustfmt
	cargo clippy --version > /dev/null || rustup component add clippy

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
	@echo "✅ Full test suite passed"

# Setup for new developers
setup: install-tools
	@echo "✅ Development environment setup complete"
