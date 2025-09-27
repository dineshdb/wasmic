use crate::config::Profile;
use crate::error::Result;
use crate::executor::WasmExecutor;
use crate::mcp::WasmMcpServer;
use crate::oci::OciManager;
use crate::wasm::WasmComponent;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, instrument};
use wasmtime::Engine;

/// MCP transport type
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// HTTP transport
    Http { host: String, port: u16 },
}

/// Server mode configuration
#[derive(Debug)]
pub enum ServerMode {
    /// Run as MCP server
    Mcp {
        profile: Profile,
        transport: McpTransport,
        engine: Arc<Engine>,
    },
    /// Direct function call
    Call {
        profile: Profile,
        function: String,
        args: String,
        engine: Arc<Engine>,
    },
    /// List available functions
    List {
        profile: Profile,
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
                profile,
                transport,
                engine,
            } => Self::run_mcp_server(profile, transport, engine).await,
            ServerMode::Call {
                profile,
                function,
                args,
                engine,
            } => Self::execute_function_call(profile, function, args, engine).await,
            ServerMode::List { profile, engine } => Self::list_functions(profile, engine).await,
        }
    }

    /// Load all components from a profile configuration into an executor (parallel and async)
    #[instrument(level = "debug", skip(profile, engine), fields(components, duratio_ms))]
    async fn load(profile: crate::config::Profile, engine: Arc<Engine>) -> Result<WasmExecutor> {
        if profile.components.is_empty() {
            return Err(crate::error::WasiMcpError::InvalidArguments(
                "Profile has no components configured".to_string(),
            ));
        }

        let oci_manager = Arc::new(OciManager::new()?);
        let mut executor = WasmExecutor::new(engine.clone(), profile.clone())?;

        // Prepare component loading tasks for parallel execution
        let load_tasks: Vec<_> = profile
            .components
            .iter()
            .map(|(name, component_config)| {
                let name = name.clone();
                let component_config = component_config.clone();
                let oci_manager = oci_manager.clone();
                let engine = engine.clone();

                async move {
                    // Resolve the component reference (handle both local and OCI)
                    let resolved_path = oci_manager
                        .resolve_component_reference(
                            component_config.path.as_deref(),
                            component_config.oci.as_deref(),
                        )
                        .await?;

                    let wasm_component = WasmComponent::new_with_engine(
                        name.clone(),
                        &resolved_path,
                        engine.clone(),
                    )?;
                    Ok::<(String, WasmComponent), crate::error::WasiMcpError>((
                        name,
                        wasm_component,
                    ))
                }
            })
            .collect();

        let start_time = Instant::now();
        // Execute all component loading tasks in parallel with concurrency limit
        let loaded_components = futures::future::try_join_all(
            load_tasks
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

        for (name, wasm_component) in loaded_components {
            executor.add_component(name, wasm_component)?;
        }

        tracing::Span::current().record("components", profile.components.len());
        tracing::Span::current().record("duration_ms", start_time.elapsed().as_millis());
        Ok(executor)
    }

    /// Run multiple WASM components from a configuration file in a single MCP server
    async fn run_mcp_server(
        profile: Profile,
        transport: McpTransport,
        engine: Arc<Engine>,
    ) -> Result<()> {
        let executor = Self::load(profile, engine).await?;
        let server = WasmMcpServer::new(executor);

        match transport {
            McpTransport::Http { host, port } => {
                tracing::info!(host, port, "Starting MCP HTTP server",);
                WasmMcpServer::serve_http(server, host, port).await?;
            }
        }
        Ok(())
    }

    /// Execute a direct function call using a configuration profile
    #[instrument(level = "debug", skip(engine, profile), fields(function_name, args))]
    async fn execute_function_call(
        profile: Profile,
        function: String,
        args: String,
        engine: Arc<Engine>,
    ) -> Result<()> {
        tracing::info!(function, args, "Executing function");

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

        // Parse the function name to extract component name
        let (component_name, _) = function.split_once('.').ok_or_else(|| {
            crate::error::WasiMcpError::InvalidArguments(format!(
                "Function name must be in format 'component.function', got: {function}"
            ))
        })?;

        let mut profile = profile.clone();
        profile.components.retain(|k, _| k == component_name);
        let executor = Self::load(profile, engine).await?;
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
        skip(engine),
        fields(profile_name, functions, components)
    )]
    async fn list_functions(profile: Profile, engine: Arc<Engine>) -> Result<()> {
        let executor = Self::load(profile.clone(), engine).await?;
        let tools = executor.get_all_tools()?;

        tracing::Span::current().record("functions", tools.len());
        tracing::Span::current().record("components", executor.list_components().len());

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
