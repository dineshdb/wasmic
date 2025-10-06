use crate::config::Profile;
use crate::error::Result;
use crate::executor::WasmExecutor;
use rmcp::model::ServerCapabilities;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, Content, GetPromptRequestParam, GetPromptResult,
        ListPromptsResult, ListToolsResult, Prompt as McpPrompt, PromptMessage,
        PromptMessageContent, PromptMessageRole, ServerInfo,
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
    pub profile: Arc<Profile>,
}

impl WasmMcpServer {
    /// Create a new WASM MCP server
    pub fn new(executor: WasmExecutor, config: Profile) -> Self {
        Self {
            executor: Arc::new(Mutex::new(executor)),
            profile: Arc::new(config),
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
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities {
                tools: Some(rmcp::model::ToolsCapability { list_changed: Some(true) }),
                prompts: Some(rmcp::model::PromptsCapability { list_changed: Some(true) }),
                ..Default::default()
            },
            server_info: rmcp::model::Implementation {
                name: "wasmic".into(),
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
        let tools = self.executor.lock().await.get_all_tools().map_err(|e| {
            tracing::error!("Failed to create tools: {}", e);
            McpError::internal_error(format!("Failed to create tools: {e}"), None)
        })?;

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
        debug!("Tool result: {}", content);
        Ok(CallToolResult::success(vec![Content::text(content)]))
    }

    /// List available prompts
    async fn list_prompts(
        &self,
        _params: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListPromptsResult, McpError> {
        let mut prompts = Vec::new();

        for (prompt_id, prompt) in &self.profile.prompts {
            prompts.push(McpPrompt {
                name: prompt_id.clone(),
                description: Some(prompt.description.clone()),
                arguments: Some(Vec::new()), // Static prompts with no arguments
                title: None,
                icons: None,
            });
        }

        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    /// Get a specific prompt
    async fn get_prompt(
        &self,
        params: GetPromptRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<GetPromptResult, McpError> {
        if let Some(prompt) = self.profile.prompts.get(&params.name) {
            return Ok(GetPromptResult {
                description: Some(prompt.description.clone()),
                messages: vec![PromptMessage {
                    role: PromptMessageRole::User,
                    content: PromptMessageContent::Text {
                        text: prompt.content.clone(),
                    },
                }],
            });
        }

        Err(McpError::invalid_params(
            format!("Prompt '{}' not found", params.name),
            None,
        ))
    }
}
