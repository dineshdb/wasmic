use crate::config::Config;
use crate::error::Result;
use crate::executor::WasmExecutor;
use rmcp::model::ServerCapabilities;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
        GetPromptRequestParams, GetPromptResponse, GetPromptResult, ListPromptsResult,
        ListToolsResult, Prompt as McpPrompt, PromptMessage, Role, ServerInfo,
    },
    service::{RequestContext, RoleServer},
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::debug;

#[derive(Clone)]
pub struct WasmMcpServer {
    pub executor: Arc<Mutex<WasmExecutor>>,
    pub config: Arc<Config>,
}

impl WasmMcpServer {
    /// Create a new WASM MCP server
    pub fn new(executor: WasmExecutor, config: Config) -> Self {
        Self {
            executor: Arc::new(Mutex::new(executor)),
            config: Arc::new(config),
        }
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
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_tool_list_changed()
            .enable_prompts()
            .enable_prompts_list_changed()
            .build();
        ServerInfo::new(capabilities)
            .with_protocol_version(rmcp::model::ProtocolVersion::V_2024_11_05)
            .with_server_info(rmcp::model::Implementation::new("wasmic", "0.2.0"))
            .with_instructions(
                "This server exposes WASM component functions as MCP tools. \
                Use the execute_wasm_tool function to call specific WASM functions. \
                The server supports named arguments and proper argument mapping for better usability. \
                Arguments should be provided as a JSON object with parameter names as keys.",
            )
    }

    /// List available tools
    async fn list_tools(
        &self,
        _params: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        let tools = self.executor.lock().await.get_all_tools().map_err(|e| {
            tracing::error!("Failed to create tools: {}", e);
            McpError::internal_error(format!("Failed to create tools: {e}"), None)
        })?;

        Ok(ListToolsResult::with_all_items(tools))
    }

    /// Call a tool (execute WASM function)
    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, McpError> {
        let arguments_map = params.arguments.unwrap_or_default();
        let arguments: HashMap<String, serde_json::Value> = arguments_map.into_iter().collect();

        let result = self
            .executor
            .lock()
            .await
            .execute_function(&params.name, arguments)
            .await
            .map_err(|e| McpError::internal_error(format!("Failed to execute tool: {e}"), None))?;

        let content = serde_json::to_string(&result).map_err(|e| {
            McpError::internal_error(format!("Failed to serialize result: {e}"), None)
        })?;
        debug!("Tool result: {content}");
        Ok(CallToolResult::success(vec![ContentBlock::text(content)]).into())
    }

    /// List available prompts
    async fn list_prompts(
        &self,
        _params: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListPromptsResult, McpError> {
        let mut prompts = Vec::new();

        for (prompt_id, prompt) in &self.config.prompts {
            prompts.push(McpPrompt::new(
                prompt_id.clone(),
                Some(prompt.description.clone()),
                Some(Vec::new()), // Static prompts with no arguments
            ));
        }

        Ok(ListPromptsResult::with_all_items(prompts))
    }

    /// Get a specific prompt
    async fn get_prompt(
        &self,
        params: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<GetPromptResponse, McpError> {
        if let Some(prompt) = self.config.prompts.get(&params.name) {
            return Ok(GetPromptResult::new(vec![PromptMessage::new_text(
                Role::User,
                prompt.content.clone(),
            )])
            .with_description(prompt.description.clone())
            .into());
        }

        Err(McpError::invalid_params(
            format!("Prompt '{}' not found", params.name),
            None,
        ))
    }
}
