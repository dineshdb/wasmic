use thiserror::Error;

pub type WasmicResult<T> = std::result::Result<T, WasmicError>;

#[derive(Error, Debug)]
pub enum WasmicError {
    #[error("WASM component error: {0}")]
    Component(#[from] wasmtime::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    #[error("Interface not found: {0}")]
    InterfaceNotFound(String),

    #[error("Component not found: {0}")]
    ComponentNotFound(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("Expected {0}, got: {1}")]
    UnexpectedExpected(String, String),
}

impl From<WasmicError> for rmcp::ErrorData {
    fn from(err: WasmicError) -> Self {
        rmcp::ErrorData::internal_error(err.to_string(), None)
    }
}
