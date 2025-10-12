use crate::error::WasmicResult;
use crate::executor::WasmExecutor;
use crate::{ComponentRunStates, WasmicError};
use crate::{config::Config, executor::ProxyMiddleware};
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
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
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower::Service;
use tower::ServiceBuilder;
use tracing::{debug, info};
use wasmtime::{Engine, Store};
use wasmtime_wasi_http::WasiHttpView;
use wasmtime_wasi_http::bindings::http::types::Scheme;
use wasmtime_wasi_http::io::TokioIo;

async fn check_auth(
    engine: Engine,
    middleware: ProxyMiddleware,
    request: &Request<Incoming>,
) -> bool {
    // Extract request parts without consuming the original request
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();
    let headers = request.headers().clone();
    let body = request.body().clone();

    // Create a new request with empty body for auth checking
    let empty_body = http_body_util::Empty::<Bytes>::new();
    let mut builder = Request::builder().method(method).uri(uri).version(version);

    for (name, value) in headers.iter() {
        builder = builder.header(name, value);
    }

    let auth_request: Request<Incoming> = builder
        .body(empty_body)
        .expect("Failed to build auth request");

    // Convert the request to a format that WASI proxy can handle
    match run_proxy(engine, middleware, auth_request).await {
        Ok(response) => {
            let status = response.status();
            tracing::debug!("Auth proxy response status: {}", status);
            status.is_success()
        }
        Err(e) => {
            tracing::error!("Auth proxy error: {:?}", e);
            false
        }
    }
}

async fn run_proxy(
    engine: Engine,
    middleware: ProxyMiddleware,
    request: Request<Incoming>,
) -> WasmicResult<Response<Full<Bytes>>> {
    // Execute auth middleware
    let state = ComponentRunStates::new();
    let mut store = Store::new(&engine, state);

    let (sender, receiver) = tokio::sync::oneshot::channel();
    let req = store
        .data_mut()
        .new_incoming_request(Scheme::Http, request)
        .expect("Failed to create incoming request");
    let out = store
        .data_mut()
        .new_response_outparam(sender)
        .expect("Failed to create response outparam");

    let auth_component = middleware.pre;
    let task = tokio::task::spawn(async move {
        let proxy = auth_component
            .instantiate_async(&mut store)
            .await
            .expect("Failed to instantiate auth component");

        proxy
            .wasi_http_incoming_handler()
            .call_handle(store, req, out)
            .await
            .map_err(|_| WasmicError::Execution("Auth component execution failed".to_string()))
    });

    match receiver.await {
        Ok(response_result) => {
            // Convert the response from BoxBody to Full<Bytes>
            match response_result {
                Ok(response) => {
                    let (parts, body) = response.into_parts();
                    let bytes = body.collect().await.unwrap_or_default().to_bytes();
                    Ok(Response::from_parts(parts, Full::new(bytes)))
                }
                Err(e) => Err(WasmicError::Execution(format!(
                    "Auth response error: {:?}",
                    e
                ))),
            }
        }
        Err(_) => {
            // The sender was dropped, check the task result
            match task.await {
                Ok(Ok(())) => Err(WasmicError::Execution(
                    "Auth component completed without setting response".to_string(),
                )),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(WasmicError::Execution(format!("Auth task failed: {}", e))),
            }
        }
    }
}

#[derive(Clone)]
pub struct WasmMcpServer {
    pub executor: Arc<Mutex<WasmExecutor>>,
    pub config: Arc<Config>,
    pub auth: Option<ProxyMiddleware>,
}

impl WasmMcpServer {
    /// Create a new WASM MCP server
    pub fn new(executor: WasmExecutor, config: Config) -> Self {
        Self {
            executor: Arc::new(Mutex::new(executor)),
            config: Arc::new(config),
            auth: None,
        }
    }

    /// Create a new WASM MCP server with authentication middleware
    pub async fn new_with_auth(mut executor: WasmExecutor, config: Config) -> WasmicResult<Self> {
        let auth_middleware = if let Some(auth_config) = &config.auth {
            // Add auth middleware to executor
            executor
                .add_middleware("auth".to_string(), auth_config.clone())
                .await?;
            Some(executor.middlewares.get("auth").cloned().ok_or_else(|| {
                WasmicError::Execution(
                    "Failed to retrieve auth middleware after adding".to_string(),
                )
            })?)
        } else {
            None
        };

        Ok(Self {
            executor: Arc::new(Mutex::new(executor)),
            config: Arc::new(config),
            auth: auth_middleware,
        })
    }

    pub async fn serve_http(service: WasmMcpServer, host: String, port: u16) -> WasmicResult<()> {
        tracing::info!(
            "Starting MCP server with HTTP transport on {}:{}",
            host,
            port
        );

        let _service = service.clone();
        let streamable_service = StreamableHttpService::new(
            move || Ok(_service.clone()),
            LocalSessionManager::default().into(),
            Default::default(),
        );

        let addr: SocketAddr = format!("{}:{}", host, port).parse().map_err(|e| {
            crate::error::WasmicError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
        })?;
        let listener = TcpListener::bind(addr).await?;
        loop {
            let service = service.clone();
            let streamable_service = streamable_service.clone();
            let (stream, _) = listener.accept().await?;

            let io = TokioIo::new(stream);
            let service_handler = service_fn(move |request: Request<Incoming>| {
                let streamable_service = streamable_service.clone();
                let server_state = service.clone();

                async move { handle_request(request, streamable_service, server_state).await }
            });
            let service_stack = ServiceBuilder::new().service(service_handler);

            // Spawn a tokio task to serve multiple connections concurrently
            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, service_stack)
                    .await
                {
                    eprintln!("Error serving connection: {:?}", err);
                }
            });
        }
    }
}

/// Handle incoming requests with authentication middleware
async fn handle_request(
    request: Request<Incoming>,
    mut streamable_service: StreamableHttpService<WasmMcpServer, LocalSessionManager>,
    server_state: WasmMcpServer,
) -> std::result::Result<Response<Full<Bytes>>, Infallible> {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start_time = Instant::now();

    // Check if authentication middleware is configured
    if let Some(auth_middleware) = &server_state.auth {
        tracing::debug!("Executing authentication middleware");

        // Get the engine from the executor
        let engine = server_state.executor.lock().await.context.engine.clone();

        if check_auth(engine, auth_middleware.clone(), &request).await {
            tracing::debug!("Authentication successful, proceeding with MCP request");
        } else {
            tracing::warn!("Authentication failed");
            let error_response = Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Full::new(Bytes::from("Unauthorized")))
                .unwrap();

            let duration = start_time.elapsed();
            info!(
                "MCP Response: {} {} - Status: {} - Duration: {:?}",
                method,
                uri,
                error_response.status(),
                duration
            );
            return Ok(error_response);
        }
    }

    let response = match streamable_service.call(request).await {
        Ok(resp) => {
            let (parts, body) = resp.into_parts();
            // Convert BoxBody to Full<Bytes>
            let bytes = body.collect().await.unwrap_or_default().to_bytes();
            Response::from_parts(parts, Full::new(bytes))
        }
        Err(e) => {
            tracing::error!("Service error: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Full::new(Bytes::from("Internal Server Error")))
                .unwrap()
        }
    };

    let duration = start_time.elapsed();
    info!(
        "MCP Response: {} {} - Status: {} - Duration: {:?}",
        method,
        uri,
        response.status(),
        duration
    );

    Ok(response)
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
        debug!("Tool result: {content}");
        Ok(CallToolResult::success(vec![Content::text(content)]))
    }

    /// List available prompts
    async fn list_prompts(
        &self,
        _params: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListPromptsResult, McpError> {
        let mut prompts = Vec::new();

        for (prompt_id, prompt) in &self.config.prompts {
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
        if let Some(prompt) = self.config.prompts.get(&params.name) {
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
