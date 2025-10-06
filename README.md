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

The default configuration file is `config.yaml` in the project root. The wasmic
CLI looks for configuration files in the following order:

1. Command-line argument (`--config`)
2. Environment variable (`WASIC_CONFIG`)
3. Default locations:
   - Linux/macOS: `~/.config/wasmic/config.yaml`
   - macOS: `~/Library/Application Support/wasmic/config.yaml`

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
        oci: ghcr.io/dineshdb/wasi-components/fetch:latest
    prompts:
      research:
        name: "Web Research"
        description: "Research a topic"
        content: |
          # Research a topic

          Use this workflow to monitor external APIs and track their performance:

          ## Tools
          - brave_search for searching the web. You can do multiple searches on a topic before responding
          - fetch.fetch for fetching links directly
```

## MCP Server Usage

### Running as MCP Server

Wasmic can run as a standalone MCP server that exposes WASM components as MCP
tools and prompts:

```bash
# Start MCP server with default config
wasmic mcp

# Start with custom config
wasmic mcp --config /path/to/config.yaml

# Start on specific host and port
wasmic mcp --http 0.0.0.0:8080
```

## Development

For development information, see [docs/development.md](docs/development.md).

## Example WASIP2 Component

For testing, you can use: `ghcr.io/yoshuawuyts/time:latest`
