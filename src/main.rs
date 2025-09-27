mod cli;
mod config;
mod error;
mod executor;
mod linker;
mod mcp;
mod oci;
mod server;
mod state;
mod wasm;

use crate::cli::{Cli, Commands};
use crate::error::Result;
use crate::server::{ServerManager, ServerMode};
use clap::Parser;
use std::sync::Arc;
use wasmtime::{Config, Engine};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    // Check if we're using stdio transport and configure logging accordingly
    let use_stdio_transport = matches!(cli.command, Commands::Mcp { stdio: true, .. });

    if use_stdio_transport {
        // For stdio transport, use JSON logging to stderr to avoid interfering with JSON-RPC on stdout
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .json()
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_file(false)
            .with_line_number(false)
            .with_current_span(false)
            .with_span_list(false)
            .with_writer(std::io::stderr)
            .init();
    } else {
        // For HTTP transport or other modes, use normal stdout/stderr logging
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }

    tracing::info!("Starting WASI-MCP");

    // Create a single shared engine
    let mut config = Config::new();
    config.async_support(true);
    let engine = Arc::new(Engine::new(&config)?);
    let config_path = cli.config.clone().unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("wasic")
            .join("config.yaml")
    });
    let profile = cli.profile.clone();
    let mode = match cli.command {
        Commands::Mcp { http, stdio } => {
            if stdio {
                tracing::info!("MCP stdio mode - profile: {:?}", profile);
                ServerMode::Mcp {
                    config: config_path.clone(),
                    profile,
                    transport: crate::server::McpTransport::Stdio,
                    engine: engine.clone(),
                }
            } else {
                // Parse host:port string
                let (host, port) = if http.contains(':') {
                    let parts: Vec<&str> = http.split(':').collect();
                    let host = if parts[0].is_empty() {
                        "127.0.0.1"
                    } else {
                        parts[0]
                    };
                    let port_str = parts[1..].join(":");
                    let port = port_str.parse().map_err(|_| {
                        tracing::error!("Error: Invalid port number in --http argument");
                        crate::error::WasiMcpError::InvalidArguments(
                            "Invalid port number in --http argument".to_string(),
                        )
                    })?;
                    (host.to_string(), port)
                } else {
                    // If no port specified, use default
                    (http, 8080)
                };

                tracing::info!(
                    "MCP HTTP mode - profile: {:?}, host: {}, port: {}",
                    profile,
                    host,
                    port
                );
                ServerMode::Mcp {
                    config: config_path.clone(),
                    profile,
                    transport: crate::server::McpTransport::Http { host, port },
                    engine: engine.clone(),
                }
            }
        }
        Commands::Call { function, args } => {
            tracing::info!("Call mode - profile: {:?}, function: {}", profile, function);

            ServerMode::Call {
                config: config_path.clone(),
                profile,
                function,
                args,
                engine: engine.clone(),
            }
        }
        Commands::List {} => ServerMode::List {
            config: config_path.clone(),
            profile,
            engine: engine.clone(),
        },
    };

    match ServerManager::run(mode).await {
        Ok(_) => {
            tracing::info!("WASI-MCP completed successfully");
            Ok(())
        }
        Err(e) => {
            tracing::error!("WASI-MCP failed: {}", e);
            Err(e)
        }
    }
}
