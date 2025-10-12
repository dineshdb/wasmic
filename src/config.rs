use crate::WasiMcpError;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Components configuration
    pub components: HashMap<String, ComponentConfig>,

    /// Prompts configuration
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub prompts: HashMap<String, Prompt>,

    /// Optional description of the configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Prompt configuration for use-case-specific guidance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    /// Human-readable name for the prompt
    pub name: String,
    /// Description of what the prompt helps with
    pub description: String,
    /// The prompt content with use case guidance
    pub content: String,
}

/// Volume mount configuration for WASI filesystem access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    /// Host path to mount (absolute path)
    pub host_path: String,
    /// Guest path where the volume will be mounted inside WASI
    pub guest_path: String,
    /// Whether the mount should be read-only (default: false)
    #[serde(default)]
    pub read_only: bool,
}

/// Individual component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentConfig {
    /// Path to the local WASM component file (mutually exclusive with oci)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// OCI reference for the WASM component (mutually exclusive with path)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oci: Option<String>,
    /// Optional configuration data for the component
    pub config: Option<serde_json::Value>,
    /// Volume mounts for filesystem access
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<VolumeMount>,
    /// Current working directory for the component
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Environment variables for the component
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub env: HashMap<String, String>,
    /// Optional description of the component
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;

        let config: Config = serde_yaml::from_str(&content).map_err(|e| {
            WasiMcpError::InvalidArguments(format!("Invalid YAML configuration: {e}",))
        })?;

        tracing::debug!(
            prompts = config.prompts.len(),
            components = config.components.len(),
            "Loaded configuration"
        );

        Ok(config)
    }
}
