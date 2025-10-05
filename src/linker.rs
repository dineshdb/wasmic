use crate::WasiMcpError;
use crate::config::ComponentConfig;
use crate::error::Result;
use crate::state::ComponentRunStates;
use std::path::Path;
use wasmtime_wasi::WasiCtxBuilder;

impl TryFrom<&ComponentConfig> for ComponentRunStates {
    type Error = WasiMcpError;

    fn try_from(config: &ComponentConfig) -> std::result::Result<Self, Self::Error> {
        let mut builder = WasiCtxBuilder::new();
        builder.inherit_stdio().inherit_args();

        // Determine the working directory
        if let Some(cwd_path) = &config.cwd {
            let path = Path::new(cwd_path);
            if !path.exists() {
                return Err(crate::error::WasiMcpError::InvalidArguments(format!(
                    "Working directory does not exist: {}",
                    cwd_path
                )));
            }
            if !path.is_dir() {
                return Err(crate::error::WasiMcpError::InvalidArguments(format!(
                    "Working directory path is not a directory: {}",
                    cwd_path
                )));
            }

            builder.preopened_dir(
                path,
                ".",
                wasmtime_wasi::DirPerms::all(),
                wasmtime_wasi::FilePerms::all(),
            )?;
        }

        // Add volume mounts to the WASI context
        for mount in &config.volumes {
            let host_path = Path::new(&mount.host_path);

            // Check if the host path exists
            if !host_path.exists() {
                return Err(crate::error::WasiMcpError::InvalidArguments(format!(
                    "Host path does not exist: {}",
                    mount.host_path
                )));
            }

            // Open the directory/file based on the host path type
            let dir_to_mount = if host_path.is_dir() {
                host_path
            } else {
                host_path.parent().ok_or_else(|| {
                    crate::error::WasiMcpError::InvalidArguments(format!(
                        "Cannot mount file without parent directory: {}",
                        mount.host_path
                    ))
                })?
            };

            // Add the preopened directory to the WASI context
            builder.preopened_dir(
                dir_to_mount,
                mount.guest_path.clone(),
                wasmtime_wasi::DirPerms::all(),
                wasmtime_wasi::FilePerms::all(),
            )?;

            tracing::debug!(
                "Mounted {} to {} (read-only: {})",
                mount.host_path,
                mount.guest_path,
                mount.read_only
            );
        }

        // Add environment variables to the WASI context
        for (key, value) in &config.env {
            builder.env(key, value);
            tracing::debug!("Set environment variable: {}={}", key, value);
        }

        let wasi_ctx = builder.build();
        Ok(ComponentRunStates {
            wasi_ctx,
            resource_table: wasmtime::component::ResourceTable::new(),
            http_ctx: wasmtime_wasi_http::WasiHttpCtx::new(),
        })
    }
}

/// Create a WASI context for component execution with volume mounts and environment variables
pub fn create_wasi_context(config: &ComponentConfig) -> Result<ComponentRunStates> {
    ComponentRunStates::try_from(config)
}
