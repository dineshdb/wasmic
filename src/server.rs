use crate::config::Config;
use crate::error::Result;
use crate::executor::WasmExecutor;
use crate::mcp::WasmMcpServer;
use crate::oci::OciManager;
use crate::wasm::WasmComponent;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, instrument};
use wasmtime::Engine;

/// MCP transport type
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// HTTP transport
    Http { host: String, port: u16 },
    /// Stdio transport
    Stdio,
}

/// Server mode configuration
#[derive(Debug)]
pub enum ServerMode {
    /// Run as MCP server
    Mcp {
        config: PathBuf,
        profile: String,
        transport: McpTransport,
        engine: Arc<Engine>,
    },
    /// Direct function call
    Call {
        config: PathBuf,
        profile: String,
        function: String,
        args: String,
        engine: Arc<Engine>,
    },
    /// List available functions
    List {
        config: PathBuf,
        profile: String,
        engine: Arc<Engine>,
    },
}

/// Manages server operations with improved error handling and caching
pub struct ServerManager;

impl ServerManager {
    /// Run the server in the specified mode
    pub async fn run(mode: ServerMode) -> Result<()> {
        match mode {
            ServerMode::Mcp {
                config,
                profile,
                transport,
                engine,
            } => Self::run_mcp_server(config, profile, transport, engine).await,
            ServerMode::Call {
                config,
                profile,
                function,
                args,
                engine,
            } => Self::execute_function_call(config, profile, function, args, engine).await,
            ServerMode::List {
                config,
                profile,
                engine,
            } => Self::list_functions(config, profile, engine).await,
        }
    }

    /// Load all components from a profile configuration into an executor (parallel and async)
    #[instrument(level = "info", skip(config_path, engine), fields(profile, components))]
    async fn load_components(
        config_path: &PathBuf,
        profile: &str,
        engine: Arc<Engine>,
    ) -> Result<WasmExecutor> {
        tracing::info!(profile, "Loading components");

        let config = Config::from_file(config_path)?;
        let profile_details = config.get_profile(profile).ok_or_else(|| {
            crate::error::WasiMcpError::InvalidArguments(format!(
                "Profile '{profile}' not found in configuration",
            ))
        })?;

        if profile_details.components.is_empty() {
            return Err(crate::error::WasiMcpError::InvalidArguments(format!(
                "Profile '{profile}' has no components configured",
            )));
        }

        let oci_manager = Arc::new(OciManager::new()?);
        let mut executor = WasmExecutor::new(engine.clone())?;

        // Prepare component loading tasks for parallel execution
        let component_load_tasks: Vec<_> = profile_details
            .components
            .iter()
            .map(|(name, component_config)| {
                let name = name.clone();
                let component_config = component_config.clone();
                let oci_manager = oci_manager.clone();
                let engine = engine.clone();

                async move {
                    let source = if let Some(oci_ref) = &component_config.oci {
                        format!("OCI: {oci_ref}")
                    } else if let Some(path) = &component_config.path {
                        format!("local: {path}")
                    } else {
                        "unknown".to_string()
                    };

                    tracing::debug!(component_name = %name, source, "Loading component");

                    // Resolve the component reference (handle both local and OCI)
                    let resolved_path = oci_manager
                        .resolve_component_reference(
                            component_config.path.as_deref(),
                            component_config.oci.as_deref(),
                        )
                        .await?;

                    // Create the WASM component
                    let wasm_component = WasmComponent::new_with_engine(
                        name.clone(),
                        &resolved_path,
                        engine.clone(),
                    )?;

                    tracing::debug!(component_name = %name, "Component loaded");

                    Ok::<(String, WasmComponent), crate::error::WasiMcpError>((
                        name,
                        wasm_component,
                    ))
                }
            })
            .collect();

        // Execute all component loading tasks in parallel with concurrency limit
        let loaded_components = futures::future::try_join_all(
            component_load_tasks
                .into_iter()
                .map(|task| tokio::spawn(task))
                .collect::<Vec<_>>(),
        )
        .await
        .map_err(|e| {
            crate::error::WasiMcpError::Execution(format!("Component loading task failed: {e}"))
        })?
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;

        // Add all loaded components to the executor in batch
        for (name, wasm_component) in loaded_components {
            executor.add_component(name, wasm_component)?;
        }

        tracing::Span::current().record("components", profile_details.components.len());
        tracing::info!(
            profile,
            components = profile_details.components.len(),
            "Successfully loaded",
        );

        Ok(executor)
    }

    /// Run multiple WASM components from a configuration file in a single MCP server
    #[instrument(level = "info", skip(config_path, engine), fields(profile))]
    async fn run_mcp_server(
        config_path: PathBuf,
        profile: String,
        transport: McpTransport,
        engine: Arc<Engine>,
    ) -> Result<()> {
        let executor = Self::load_components(&config_path, &profile, engine).await?;
        let server = WasmMcpServer::new(executor);

        match transport {
            McpTransport::Http { host, port } => {
                tracing::info!(profile, host, port, "Starting MCP HTTP server",);
                WasmMcpServer::serve_http(server, host, port).await?;
            }
            McpTransport::Stdio => {
                tracing::info!(profile, "Starting MCP stdio server",);
                server.serve_stdio().await?;
            }
        }
        Ok(())
    }

    /// Execute a direct function call using a configuration profile
    #[instrument(
        level = "info",
        skip(config_path, engine),
        fields(profile_name, function_name, args)
    )]
    async fn execute_function_call(
        config_path: PathBuf,
        profile_name: String,
        function: String,
        args: String,
        engine: Arc<Engine>,
    ) -> Result<()> {
        tracing::info!(profile_name, function_name = %function, args, "Executing function call: {} for profile: {}", function, profile_name);

        // Parse arguments as named arguments (JSON object)
        let arguments: HashMap<String, serde_json::Value> = serde_json::from_str(&args)
            .map_err(|e| {
                tracing::warn!("Failed to parse arguments as JSON object, using empty map: {e}");
                crate::error::WasiMcpError::InvalidArguments(
                    format!("Invalid JSON arguments: {e}. Expected a JSON object with parameter names as keys, e.g., {{\"param1\": \"value1\", \"param2\": \"value2\"}}",),
                )
            })
            .unwrap_or_default();

        tracing::debug!(parsed_args_count = %arguments.len(), "Arguments parsed");

        let executor = Self::load_components(&config_path, &profile_name, engine).await?;
        let result = executor.execute_function(&function, arguments).await;

        match result {
            Ok(result) => {
                let output = serde_json::to_string_pretty(&result).map_err(|e| {
                    tracing::error!("Failed to serialize result: {}", e);
                    crate::error::WasiMcpError::Json(e)
                })?;

                info!("{output}",);
                tracing::info!("Function execution completed successfully");
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "Error executing function");
                return Err(e);
            }
        }
    }

    /// List available functions from all components in a configuration file
    #[instrument(
        level = "info",
        skip(config_path, engine),
        fields(profile_name, functions, components)
    )]
    async fn list_functions(
        config_path: PathBuf,
        profile: String,
        engine: Arc<Engine>,
    ) -> Result<()> {
        tracing::info!(profile, "Listing functions",);

        let executor = Self::load_components(&config_path, &profile, engine).await?;
        let tools = executor.get_all_tools()?;

        tracing::Span::current().record("functions", tools.len());
        tracing::Span::current().record("components", executor.list_components().len());

        info!(profile, "All functions:",);
        for tool in &tools {
            info!(
                "  - {}: {}",
                tool.name,
                tool.description.as_deref().unwrap_or("No description")
            );
            debug!("Function details: {:?}", tool);
        }

        tracing::info!(
            "Listed {} functions from {} components",
            tools.len(),
            executor.list_components().len()
        );
        Ok(())
    }
}
