use crate::error::Result;
use oci_distribution::Reference;
use oci_distribution::client::{Client, ClientConfig, ClientProtocol};
use oci_distribution::secrets::RegistryAuth;
use std::fs;
use std::path::PathBuf;
use tokio::fs as tokio_fs;
use tokio::io::AsyncWriteExt;
use tracing::instrument;

/// OCI artifact manager for downloading and caching WASM components
pub struct OciManager {
    client: Client,
    cache_dir: PathBuf,
}

impl OciManager {
    /// Create a new OCI manager with XDG cache directory
    pub fn new() -> Result<Self> {
        let cache_dir = Self::get_cache_dir()?;
        fs::create_dir_all(&cache_dir)?;

        let client_config = ClientConfig {
            protocol: ClientProtocol::Https,
            ..Default::default()
        };

        let client = Client::new(client_config);

        Ok(Self { client, cache_dir })
    }

    /// Get XDG cache directory for wasmic
    fn get_cache_dir() -> Result<PathBuf> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| {
                crate::error::WasiMcpError::InvalidArguments(
                    "Could not determine cache directory".to_string(),
                )
            })?
            .join("wasmic");

        Ok(cache_dir)
    }

    /// Download and cache a WASM component from OCI registry with optimized caching
    #[instrument(level = "debug", skip(self), fields(reference, duration_ms))]
    pub async fn download_wasm_component(&self, reference: &str) -> Result<PathBuf> {
        let start_time = std::time::Instant::now();

        let parsed_ref = Reference::try_from(reference).map_err(|e| {
            crate::error::WasiMcpError::InvalidArguments(format!(
                "Invalid OCI reference '{reference}': {e}"
            ))
        })?;

        // Create a unique filename based on the reference and digest
        let cache_key = parsed_ref.whole().replace("/", "_").replace(":", "_");
        let cached_path = self.cache_dir.join(format!("{cache_key}.wasm"));

        // Check if the artifact is already cached - cache is valid forever
        if cached_path.exists() {
            tracing::debug!("Using cached WASM component: {:?}", cached_path);
            return Ok(cached_path);
        }

        tracing::info!("Downloading WASM component from OCI: {}", reference);

        // Pull the image content
        let image_content = self
            .client
            .pull(
                &parsed_ref,
                &RegistryAuth::Anonymous,
                vec![
                    "application/vnd.wasm.content.layer.v1+wasm",
                    "application/wasm",
                ],
            )
            .await
            .map_err(|e| {
                crate::error::WasiMcpError::InvalidArguments(format!(
                    "Failed to pull OCI artifact '{reference}': {e}"
                ))
            })?;

        // Find the WASM layer
        let wasm_layer = image_content
            .layers
            .into_iter()
            .find(|layer| {
                layer.media_type == "application/vnd.wasm.content.layer.v1+wasm"
                    || layer.media_type == "application/wasm"
            })
            .ok_or_else(|| {
                crate::error::WasiMcpError::InvalidArguments(
                    "No WASM layer found in OCI artifact".to_string(),
                )
            })?;

        // Write the WASM file to cache
        let mut file = tokio_fs::File::create(&cached_path).await?;
        file.write_all(&wasm_layer.data).await?;

        tracing::Span::current().record("duration_ms", start_time.elapsed().as_millis());
        Ok(cached_path)
    }

    /// Resolve a component reference to a local file path (downloading from OCI if necessary)
    pub async fn resolve_component_reference(
        &self,
        component_path: Option<&str>,
        component_oci: Option<&str>,
    ) -> Result<PathBuf> {
        match (component_path, component_oci) {
            (Some(path), None) => Ok(PathBuf::from(path)),
            (None, Some(oci_ref)) => self.download_wasm_component(oci_ref).await,
            (Some(_), Some(_)) => Err(crate::error::WasiMcpError::InvalidArguments(
                "Cannot specify both 'path' and 'oci' for the same component".to_string(),
            )),
            (None, None) => Err(crate::error::WasiMcpError::InvalidArguments(
                "Must specify either 'path' or 'oci' for component".to_string(),
            )),
        }
    }
}
