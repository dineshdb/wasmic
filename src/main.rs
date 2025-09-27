use clap::Parser;
use std::sync::Arc;
use tracing::error;
use wasic::WasiMcpError;
use wasic::cli::{Cli, Commands};
use wasic::config::Config;
use wasic::error::Result;
use wasic::server::{ServerManager, ServerMode};
use wasmtime::Engine;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    // Configure normal stdout/stderr logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting WASI-MCP");

    // Create a single shared engine
    let mut config = wasmtime::Config::new();
    config.async_support(true);
    let engine = Arc::new(Engine::new(&config)?);
    let config_path = cli.config.clone().unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("wasic")
            .join("config.yaml")
    });
    let config = Config::from_file(&config_path)?;
    let profile = config
        .profiles
        .get(&cli.profile)
        .ok_or_else(|| {
            WasiMcpError::InvalidArguments(format!(
                "Profile '{}' not found in configuration",
                cli.profile
            ))
        })?
        .clone();
    let mode = match cli.command {
        Commands::Mcp { http } => {
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
                    error!("Error: Invalid port number in --http argument");
                    WasiMcpError::InvalidArguments(
                        "Invalid port number in --http argument".to_string(),
                    )
                })?;
                (host.to_string(), port)
            } else {
                // If no port specified, use default
                (http, 8080)
            };

            tracing::debug!(
                "MCP HTTP mode - profile: {:?}, host: {}, port: {}",
                profile,
                host,
                port
            );
            ServerMode::Mcp {
                profile,
                transport: wasic::server::McpTransport::Http { host, port },
                engine: engine.clone(),
            }
        }
        Commands::Call { function, args } => ServerMode::Call {
            profile,
            function,
            args,
            engine: engine.clone(),
        },
        Commands::List {} => ServerMode::List {
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
