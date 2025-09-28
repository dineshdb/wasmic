# Development Guide

This document provides comprehensive information for developers working on the
wasmic project.

## Prerequisites

- Rust 1.88.0 or later
- cargo-component
- wasm-tools
- wkg
- just (command runner)

### Installing wasmic (Recommended)

For users and developers, the recommended way to install wasmic is using
`cargo binstall`:

```bash
cargo binstall wasmic
```

### Installing Development Dependencies

```bash
# Install required tools
just install-tools

# Verify installation
just check-tools
```

## Project Structure

```
wasmic/
├── pkg/
│   ├── wasmic/           # Main CLI application
│   ├── time/           # Time WASM component
│   └── fetch/          # Fetch WASM component
├── config.yaml         # Default configuration
├── Justfile           # Command definitions
└── docs/              # Documentation
```

## Development Workflow

### 1. Setup Development Environment

```bash
# Clone and setup
git clone <repository-url>
cd wasmic

# Install tools and build
just setup
```

### 2. Development Commands

```bash
# Development workflow (build + validate + extract WIT)
just dev

# Quick test (build + validate WASM)
just quick-test

# Full CI suite locally
just ci

# Run specific tests
just test
just test-verbose

# Code quality checks
just lint
just lint-fix

# Format code
just fmt
```

### 3. Building Components

```bash
# Build all WASM components
just build

# Build specific component
just _build time
just _build fetch
```

### 4. Testing Components

```bash
# Validate WASM components
just validate-wasm

# Extract WIT interfaces
just extract-wit

# Inspect specific components
just inspect-time
just inspect-fetch
```

## Code Quality

### Formatting and Linting

```bash
# Format code
just fmt

# Check formatting
just fmt-check

# Run linter
just clippy

# Auto-fix linting issues
just clippy-fix

# Check for unused dependencies
just machete

# Sort dependencies
just sort

# Run all quality checks
just lint
```

### Testing

```bash
# Run all tests
just test

# Run tests with verbose output
just test-verbose

# Run tests for specific package
cargo test -p wasmic
```

## WASM Component Development

### Creating New Components

1. Create new package in `pkg/` directory
2. Add WIT interface definition in `wit/` subdirectory
3. Implement component logic in `src/lib.rs`
4. Add component to `Justfile` build process
5. Update `config.yaml` to include the new component

### Component Structure

```
pkg/mycomponent/
├── Cargo.toml
├── wit/
│   └── mycomponent.wit
└── src/
    ├── lib.rs
    └── bindings.rs  # Auto-generated
```

### Building Components

```bash
# Generate bindings
cd pkg/mycomponent && cargo component bindings

# Build component
cd pkg/mycomponent && cargo build --target wasm32-wasip2 --release
```

## Configuration

### Default Configuration File

The default configuration file is `config.yaml` in the project root. It defines:

- Profiles for different environments
- Component configurations
- OCI registry settings
- MCP server settings

### Configuration Locations

The wasmic CLI looks for configuration files in the following order:

1. Command-line argument (`--config`)
2. Environment variable (`WASIC_CONFIG`)
3. Default locations:
   - Linux/macOS: `~/.config/wasmic/config.yaml`
   - macOS: `~/Library/Application Support/wasmic/config.yaml`

### Cache Folder

Wasic uses a cache folder for storing downloaded OCI artifacts and other
temporary files:

- Linux: `~/.cache/wasmic/`
- macOS: `~/Library/Caches/wasmic/`
- Windows: `%LOCALAPPDATA%\wasmic\cache\`

## MCP Server Integration

### Running as MCP Server

```bash
# Start MCP server with default config
wasmic server

# Start with custom config
wasmic server --config /path/to/config.yaml

# Start in debug mode
wasmic server --debug
```

### MCP Server Configuration

The MCP server is configured in the `config.yaml` file:

```yaml
server:
  host: "127.0.0.1"
  port: 8080
  debug: false

mcp:
  server_name: "wasmic"
  version: "1.0.0"
```

### Testing MCP Server

```bash
# Test server health
curl http://localhost:8080/health

# Test component listing
curl http://localhost:8080/components

# Test component execution
curl -X POST http://localhost:8080/call \
  -H "Content-Type: application/json" \
  -d '{"function": "time.get-current-time", "args": {}}'
```

## Release Process

### Preparing for Release

```bash
# Run all checks
just release-prep

# Verify everything works
just ci
```

### Manual Release

1. Ensure all tests pass
2. Update version in package.json if needed
3. Trigger release workflow from GitHub Actions
4. Select release type (patch/minor/major)
5. Monitor release process

### Release Artifacts

The release process creates:

- GitHub Release with binaries for all platforms
- Cargo package publication
- WASM components published to OCI registry
- Automatic version tagging

## Debugging

### Common Issues

1. **Component Loading Failures**
   ```bash
   # Check component paths
   wasmic --config config.yaml list

   # Validate WASM components
   just validate-wasm
   ```

2. **OCI Artifact Issues**
   ```bash
   # Clear cache
   just clean-cache

   # Test OCI pull
   just pull ghcr.io/yoshuawuyts/time:latest time
   ```

3. **MCP Server Issues**
   ```bash
   # Start server in debug mode
   wasmic server --debug

   # Check server logs
   tail -f ~/.local/share/wasmic/server.log
   ```

### Debug Commands

```bash
# Show component information
wasmic --config config.yaml list

# Test component execution
wasmic --config config.yaml call --function "time.get-current-time" --args "{}"

# Validate configuration
wasmic --config config.yaml validate
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run `just ci` to ensure all checks pass
5. Submit a pull request
6. Ensure CI passes on your PR

## Useful Commands

```bash
# Clean build artifacts
just clean

# Clean cache
just clean-cache

# Show available Just commands
just --list

# Build and run immediately
just build && cargo run --bin wasmic -- --config config.yaml list
```

## Troubleshooting

### Build Issues

```bash
# Update Rust toolchain
rustup update

# Clean and rebuild
just clean && just build

# Check for outdated dependencies
cargo outdated
```

### Tool Installation Issues

```bash
# Reinstall tools
just install-tools

# Check tool versions
cargo-component --version
wasm-tools --version
wkg --version
```

### Configuration Issues

```bash
# Validate configuration
wasmic --config config.yaml validate

# Show effective configuration
wasmic --config config.yaml show-config
```
