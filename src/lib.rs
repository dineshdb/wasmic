//! WASI Component Manager - Library
//!
//! This library provides functionality for managing WASI components and running them as MCP servers.

pub mod cli;
pub mod config;
pub mod error;
pub mod executor;
pub mod linker;
pub mod mcp;
pub mod oci;
pub mod server;
pub mod state;
mod utils;
pub mod wasm;

// Re-export commonly used types
pub use config::{ComponentConfig, Config, VolumeMount};
pub use error::{Result, WasiMcpError};
pub use state::ComponentRunStates;
