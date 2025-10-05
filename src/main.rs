use clap::Parser;
use tracing::error;
use wasmic::WasiMcpError;
use wasmic::cli::{Cli, Commands};
use wasmic::config::Config;
use wasmic::error::Result;
use wasmic::server::{ServerManager, ServerMode};
use wasmic::wasm::WasmContext;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    // Configure normal stdout/stderr logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting WASI-MCP");

    let context = WasmContext::new()?;
    let config_path = cli.config.clone().unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("wasmic")
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
                transport: wasmic::server::McpTransport::Http { host, port },
                context,
            }
        }
        Commands::Call { function, args } => ServerMode::Call {
            profile,
            function,
            args,
            context,
        },
        Commands::List {} => ServerMode::List { profile, context },
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
