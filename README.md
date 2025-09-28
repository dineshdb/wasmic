# wasmic

A CLI tool for managing WASI components and running them as MCP (Model Context
Protocol) servers.
[See components for use](https://github.com/dineshdb/wasi-components/)

## What it does

**Wasic** enables you to run WebAssembly components as MCP (Model Context
Protocol) servers, giving you access to a wide range of tools and functionality
through a unified interface. With wasmic, you can:

- **Deploy and manage WebAssembly tools** from local files or remote OCI
  registries
- **Expose WASM component functions** as MCP tools that can be used by AI
  assistants and other MCP clients
- **Run sandboxed, cross-platform tools** without worrying about installation
  conflicts or system dependencies
- **Integrate with AI workflows** by making WebAssembly capabilities available
  through the Model Context Protocol
- **Configure and customize** component behavior through simple YAML
  configuration files

## Features

- Accepts both local file paths and OCI URLs for WASM components
- Uses wasm-tools and wkg for component manipulation
- Built on wasmtime for executing WASM components
- Integrates with MCP (Model Context Protocol) servers
- Supports component configuration and profiling

## Installation

### From GitHub Releases (Recommended)

Pre-compiled binaries for Linux (x86_64), macOS (x86_64 and ARM64), and Windows
(x86_64) are available on the
[GitHub Releases](https://github.com/dineshdb/wasmic/releases) page. This is the
recommended way to install `wasmic` for most users.

Download the appropriate archive for your system, extract it, and place the
`wasmic` (or `wasmic.exe` on Windows) binary in your system's PATH.

### Using cargo-binstall

For easy installation from crates.io (once published):

```bash
cargo binstall wasmic
```

### From Cargo

To install the latest version from source via Cargo:

```bash
cargo install wasmic
```

### From Source

To install the latest development version from source:

```bash
# Clone the repository
git clone https://github.com/dineshdb/wasmic.git
cd wasmic

# Install the CLI tool
just install
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

## Usage

### Basic Commands

```bash
# List available components
wasmic --config config.yaml list

# Call a function on a component
wasmic --config config.yaml call --function "time.get-current-time" --args "{}"

# Call fetch function
wasmic --config config.yaml call --function "fetch.fetch" --args '{"url":"https://httpbin.org/get"}'
```

### Configuration

Create a `config.yaml` file to define your components:

```yaml
profiles:
  default:
    components:
      time:
        path: target/wasm32-wasip2/release/time.wasm
        config:
          timezone: "UTC"
      fetch:
        path: target/wasm32-wasip2/release/fetch.wasm
```

Or use OCI references:

```yaml
profiles:
  default:
    components:
      time:
        oci: ghcr.io/dineshdb/wasmic-components/time:v0.1.0
        config:
          timezone: "UTC"
      fetch:
        oci: ghcr.io/dineshdb/wasmic-components/fetch:v0.1.0
```

### Building Components

```bash
# Build all WASM components
just build

# Build specific component
just _build time
just _build fetch
```

## MCP Server Usage

### Running as MCP Server

Wasic can run as a standalone MCP server that exposes WASM components as MCP
tools:

```bash
# Start MCP server with default config
wasmic mcp

# Start with custom config
wasmic mcp --config /path/to/config.yaml

# Start on specific host and port
wasmic mcp --http 0.0.0.0:8080
```

### MCP Integration

Wasic integrates with MCP (Model Context Protocol) to expose WASM components as
tools that can be used by MCP clients. The server automatically:

- Loads components from the configuration
- Generates tool definitions based on WIT interfaces
- Handles tool execution requests
- Manages component lifecycle and state

## Example Components

### Time Component

- Function: `get-current-time`
- Returns: Current time as string
- Configurable timezone support

### Fetch Component

- Function: `fetch`
- Input: URL string
- Returns: HTTP response content as string

## Dependencies

- **wkg**: For OCI artifact pulling
- **wasm-tools**: For WASM component manipulation
- **wasmtime**: For WASM runtime execution
- **rmcp**: For MCP server functionality
- **clap**: For command-line interface

## Development

For development information, see [docs/development.md](docs/development.md).

## Example WASIP2 Component

For testing, you can use: `ghcr.io/yoshuawuyts/time:latest`
