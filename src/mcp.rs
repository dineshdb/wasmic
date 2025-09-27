use crate::error::{Result, WasiMcpError};
use crate::executor::WasmExecutor;
use rmcp::ServiceExt;
use rmcp::model::ServerCapabilities;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    model::{CallToolRequestParam, CallToolResult, Content, ListToolsResult, ServerInfo},
    service::{RequestContext, RoleServer},
};
use std::sync::Arc;
use std::time::Instant;

/// WASM MCP server with improved error handling and logging
#[derive(Clone)]
pub struct WasmMcpServer {
    pub executor: Arc<WasmExecutor>,
}

impl WasmMcpServer {
    /// Create a new WASM MCP server
    pub fn new(executor: WasmExecutor) -> Self {
        Self {
            executor: Arc::new(executor),
        }
    }

    /// Serve the MCP server over stdio transport
    pub async fn serve_stdio(self) -> Result<()> {
        let start_time = Instant::now();

        // The serve method returns a RunningService that runs until completion
        let _running_service = self.serve(rmcp::transport::stdio()).await.map_err(|e| {
            tracing::error!("Failed to serve MCP: {}", e);
            WasiMcpError::Mcp(format!("Failed to serve MCP: {e}"))
        })?;

        // The service will run until completion or cancellation
        let serve_duration = start_time.elapsed();
        tracing::info!("MCP server service completed in {:?}", serve_duration);

        Ok(())
    }

    /// Serve the MCP server over HTTP transport using axum
    pub async fn serve_http(service: WasmMcpServer, host: String, port: u16) -> Result<()> {
        tracing::info!(
            "Starting MCP server with HTTP transport on {}:{}",
            host,
            port
        );

        let start_time = Instant::now();

        let service = StreamableHttpService::new(
            move || Ok(service.clone()),
            LocalSessionManager::default().into(),
            Default::default(),
        );

        let router = axum::Router::new().nest_service("/mcp", service);
        let tcp_listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
        axum::serve(tcp_listener, router)
            .with_graceful_shutdown(async { tokio::signal::ctrl_c().await.unwrap() })
            .await?;

        tracing::info!("MCP HTTP server listening on {}:{}", host, port);
        let serve_duration = start_time.elapsed();
        tracing::info!("MCP HTTP server service completed in {:?}", serve_duration);

        Ok(())
    }
}

impl ServerHandler for WasmMcpServer {
    /// Get server information
    fn get_info(&self) -> ServerInfo {
        tracing::debug!("Serving server info");
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability { list_changed: Some(true) }),
                ..Default::default()
            },
            server_info: rmcp::model::Implementation {
                name: "wasic".into(),
                version: "0.1.0".into(),
                title: None,
                website_url: None,
                icons: None,
            },
            instructions: Some(
                "This server exposes WASM component functions as MCP tools. \
                Use the execute_wasm_tool function to call specific WASM functions. \
                The server supports named arguments and proper argument mapping for better usability. \
                Arguments should be provided as a JSON object with parameter names as keys."
                    .into(),
            ),
        }
    }

    /// List available tools
    async fn list_tools(
        &self,
        _params: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        tracing::debug!("Listing tools from all components");

        let start_time = Instant::now();

        let tools = self.executor.get_all_tools().map_err(|e| {
            tracing::error!("Failed to create tools: {}", e);
            McpError::internal_error(format!("Failed to create tools: {e}"), None)
        })?;

        let list_duration = start_time.elapsed();
        tracing::info!(
            "Listed {} tools from all components in {:?}",
            tools.len(),
            list_duration
        );

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    /// Call a tool (execute WASM function)
    async fn call_tool(
        &self,
        params: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let tool_name = params.name.clone();
        let arguments_map = params.arguments.unwrap_or_default();

        // Convert serde_json::Map to HashMap<String, serde_json::Value>
        let arguments_hashmap: std::collections::HashMap<String, serde_json::Value> =
            arguments_map.into_iter().collect();

        tracing::info!(
            "Calling tool: {} with {} named arguments",
            tool_name,
            arguments_hashmap.len()
        );
        tracing::debug!("Tool named arguments: {:?}", arguments_hashmap);

        let start_time = Instant::now();

        // Pass named arguments directly to the executor
        match self
            .executor
            .execute_function(&tool_name, arguments_hashmap)
            .await
        {
            Ok(result) => {
                let execution_time = start_time.elapsed();
                tracing::info!("Tool execution successful in {:?}", execution_time);
                tracing::debug!("Tool result: {}", result.result);

                let content = serde_json::to_string(&result).map_err(|e| {
                    tracing::error!("Failed to serialize result: {}", e);
                    McpError::internal_error(format!("Failed to serialize result: {e}"), None)
                })?;

                Ok(CallToolResult::success(vec![Content::text(content)]))
            }
            Err(e) => {
                let execution_time = start_time.elapsed();
                tracing::error!("Tool execution failed after {:?}: {}", execution_time, e);

                Err(McpError::internal_error(
                    format!("Failed to execute tool: {e}"),
                    None,
                ))
            }
        }
    }
}
