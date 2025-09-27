use std::path::PathBuf;

use clap::{Parser, Subcommand, command};

#[derive(Parser)]
#[command(name = "wasi-mcp")]
#[command(about = "A tool to expose WASM components as MCP servers")]
#[command(version, propagate_version = true)]
pub struct Cli {
    /// Profile name to use when using config (defaults to "default")
    #[arg(long, default_value = "default", global = true)]
    pub profile: String,

    /// Path to the configuration file (required)
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run the WASM component as an MCP server
    Mcp {
        /// Use HTTP transport with host:port (e.g., "127.0.0.1:8080" or ":8080")
        #[arg(long, default_value = "127.0.0.1:8080")]
        http: String,
    },
    /// Directly call a WASM method
    Call {
        /// Function name in format 'component.function'
        #[arg(short, long)]
        function: String,

        /// Arguments as JSON string
        #[arg(short, long, default_value = "{}")]
        args: String,
    },
    /// List available functions in a WASM component
    List {},
}
