use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Profile definitions
    pub profiles: HashMap<String, Profile>,
}

/// Profile configuration containing multiple WASM components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Map of component names to their configurations
    pub components: HashMap<String, ComponentConfig>,
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
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;

        let config: Config = serde_yaml::from_str(&content).map_err(|e| {
            crate::error::WasiMcpError::InvalidArguments(
                format!("Invalid YAML configuration: {e}",),
            )
        })?;

        tracing::info!(
            "Loaded configuration with {} profiles",
            config.profiles.len()
        );
        for (name, profile) in &config.profiles {
            tracing::debug!(
                "Profile '{}' has {} components",
                name,
                profile.components.len()
            );
        }

        Ok(config)
    }

    /// Get a specific profile by name
    pub fn get_profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }
}
