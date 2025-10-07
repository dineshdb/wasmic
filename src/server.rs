use crate::error::Result;
use crate::executor::WasmExecutor;
use crate::mcp::WasmMcpServer;
use crate::oci::OciManager;
use crate::{ComponentConfig, WasiMcpError};
use crate::{config::Config, wasm::WasmContext};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, instrument, trace};

/// MCP transport type
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// HTTP transport
    Http { host: String, port: u16 },
}

/// Server mode configuration
pub enum ServerMode {
    /// Run as MCP server
    Mcp {
        config: Config,
        transport: McpTransport,
        context: WasmContext,
    },
    /// Direct function call
    Call {
        config: Config,
        function: String,
        args: String,
        context: WasmContext,
    },
    /// List available functions
    List {
        config: Config,
        context: WasmContext,
    },
}

pub struct ServerManager;

impl ServerManager {
    /// Run the server in the specified mode
    pub async fn run(mode: ServerMode) -> Result<()> {
        match mode {
            ServerMode::Mcp {
                config,
                transport,
                context,
            } => Self::run_mcp_server(config, transport, context).await,
            ServerMode::Call {
                config,
                function,
                args,
                context,
            } => Self::execute_function_call(config, &function, args, context).await,
            ServerMode::List { config, context } => Self::list_functions(config, context).await,
        }
    }

    #[instrument(
        level = "debug",
        skip(config, context),
        fields(components, duration_ms)
    )]
    async fn init(config: Config, context: WasmContext) -> Result<WasmExecutor> {
        if config.components.is_empty() {
            return Err(WasiMcpError::InvalidArguments(
                "Configuration has no components configured".to_string(),
            ));
        }

        let start_time = Instant::now();
        let mut executor = WasmExecutor::new(context, config.clone())?;

        let component_config = Self::load(&config).await?;
        for (name, config) in component_config {
            executor.add_component(name, config).await?;
        }

        tracing::Span::current().record("components", config.components.len());
        tracing::Span::current().record("duration_ms", start_time.elapsed().as_millis());
        Ok(executor)
    }

    /// Load all components from a configuration into an executor (parallel and async)
    #[instrument(level = "debug", skip(config), fields(components, duratio_ms))]
    async fn load(config: &Config) -> Result<Vec<(String, ComponentConfig)>> {
        if config.components.is_empty() {
            return Err(WasiMcpError::InvalidArguments(
                "Configuration has no components configured".to_string(),
            ));
        }

        let oci_manager = Arc::new(OciManager::new()?);
        // Prepare component loading tasks for parallel execution
        let load_tasks: Vec<_> = config
            .components
            .iter()
            .map(|(name, component_config)| {
                let name = name.clone();
                let mut component_config = component_config.clone();
                let oci_manager = oci_manager.clone();

                async move {
                    // Resolve the component reference (handle both local and OCI)
                    let resolved_path = oci_manager
                        .resolve_component_reference(
                            component_config.path.as_deref(),
                            component_config.oci.as_deref(),
                        )
                        .await?;
                    component_config.path = Some(resolved_path.to_string_lossy().to_string());
                    Ok::<(String, ComponentConfig), WasiMcpError>((name, component_config))
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
        .map_err(|e| WasiMcpError::Execution(format!("Component loading task failed: {e}")))?
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;

        tracing::Span::current().record("duration_ms", start_time.elapsed().as_millis());
        Ok(loaded_components)
    }

    /// Run multiple WASM components from a configuration file in a single MCP server
    async fn run_mcp_server(
        config: Config,
        transport: McpTransport,
        context: WasmContext,
    ) -> Result<()> {
        let executor = Self::init(config.clone(), context).await?;
        let server = WasmMcpServer::new(executor, config);

        match transport {
            McpTransport::Http { host, port } => {
                tracing::info!(host, port, "Starting MCP HTTP server",);
                WasmMcpServer::serve_http(server, host, port).await?;
            }
        }
        Ok(())
    }

    #[instrument(level = "debug", skip(context, config), fields(function_name, args))]
    async fn execute_function_call(
        config: Config,
        function: &str,
        args: String,
        context: WasmContext,
    ) -> Result<()> {
        tracing::info!(function, args, "Executing function");

        // Parse arguments as named arguments (JSON object)
        let arguments: HashMap<String, serde_json::Value> = serde_json::from_str(&args)
            .map_err(|e| {
                tracing::warn!("Failed to parse arguments as JSON object, using empty map: {e}");
                WasiMcpError::InvalidArguments(
                    format!("Invalid JSON arguments: {e}. Expected a JSON object with parameter names as keys, e.g., {{\"param1\": \"value1\", \"param2\": \"value2\"}}",),
                )
            })
            .unwrap_or_default();

        tracing::debug!(parsed_args_count = %arguments.len(), "Arguments parsed");

        // Parse the function name to extract component name
        let (component_name, _) = function.split_once('.').ok_or_else(|| {
            WasiMcpError::InvalidArguments(format!(
                "Function name must be in format 'component.function', got: {function}"
            ))
        })?;

        let mut config = config.clone();
        config.components.retain(|k, _| k == component_name);
        let mut executor = Self::init(config, context).await?;
        let result = executor.execute_function(function, arguments).await;

        match result {
            Ok(result) => {
                let output = serde_json::to_string_pretty(&result).map_err(|e| {
                    tracing::error!("Failed to serialize result: {}", e);
                    WasiMcpError::Json(e)
                })?;

                trace!("{output}",);
                debug!("Function execution completed successfully");
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "Error executing function");
                return Err(e);
            }
        }
    }

    #[instrument(level = "debug", skip(context, config), fields(functions, components))]
    async fn list_functions(config: Config, context: WasmContext) -> Result<()> {
        let executor = Self::init(config.clone(), context).await?;
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
        Ok(())
    }
}
