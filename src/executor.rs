use crate::config::{ComponentConfig, Config};
use crate::error::{Result, WasiMcpError};
use crate::utils::transform::{convert_args_to_wasm_values, convert_wasm_results_to_json};
use crate::wasm::{FunctionInfo, WasmComponent, WasmContext};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;
use tracing::instrument;

pub struct WasmExecutor {
    context: WasmContext,
    components: HashMap<String, WasmComponent>,
    config: Config,
}

impl WasmExecutor {
    pub fn new(context: WasmContext, config: Config) -> Result<Self> {
        Ok(Self {
            context,
            components: HashMap::new(),
            config,
        })
    }

    #[instrument(level = "debug", skip(self, config), fields(name, tools))]
    pub async fn add_component(&mut self, name: String, config: ComponentConfig) -> Result<()> {
        let component = WasmComponent::new(
            name.clone(),
            self.context.engine.clone(),
            config,
            &mut self.context.linker,
        )
        .await?;
        self.components.insert(name, component);
        Ok(())
    }

    /// Get component configuration for a specific component
    fn get_component_config(&self, component_name: &str) -> Option<&ComponentConfig> {
        self.config.components.get(component_name)
    }

    /// Get all tools from all components
    pub fn get_all_tools(&self) -> Result<Vec<rmcp::model::Tool>> {
        let mut all_tools = Vec::new();

        for (name, component) in &self.components {
            let config = self.get_component_config(name);
            let description = config.and_then(|config| config.description.as_deref());
            let mut tools = component.get_tools(&self.context.engine, description)?;

            // Prefix tool names with component name to avoid conflicts
            for tool in &mut tools {
                tool.name = format!("{name}.{}", tool.name).into();
            }

            all_tools.extend(tools);
        }

        Ok(all_tools)
    }

    /// Map named arguments to positional arguments based on function signature
    fn map_named_to_positional_arguments(
        &self,
        function_info: &FunctionInfo,
        named_args: &HashMap<String, serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut positional_args = Vec::with_capacity(function_info.params.len());

        // Create a map of parameter names to their positions for quick lookup
        let param_positions: HashMap<&str, usize> = function_info
            .params
            .iter()
            .map(|p| (p.name.as_str(), p.position))
            .collect();

        // Check for missing required arguments
        for param_info in &function_info.params {
            if !named_args.contains_key(&param_info.name) {
                return Err(WasiMcpError::InvalidArguments(format!(
                    "Missing required argument: '{}' (position: {})",
                    param_info.name, param_info.position
                )));
            }
        }

        // Check for extra arguments that aren't in the function signature
        for arg_name in named_args.keys() {
            if !param_positions.contains_key(arg_name.as_str()) {
                return Err(WasiMcpError::InvalidArguments(format!(
                    "Unexpected argument: '{arg_name}'"
                )));
            }
        }

        // Initialize positional arguments with null values
        positional_args.resize(function_info.params.len(), serde_json::Value::Null);

        // Map arguments to their correct positions
        for (arg_name, arg_value) in named_args {
            if let Some(&position) = param_positions.get(arg_name.as_str())
                && position < positional_args.len()
            {
                positional_args[position] = arg_value.clone();
            }
        }

        Ok(positional_args)
    }

    /// Execute a function from any of the managed components with named arguments (async with direct handles)
    #[instrument(level = "debug", skip(self), fields(tool_name, arguments, duration_ms))]
    pub async fn execute_function(
        &mut self,
        tool_name: &str,
        arguments: HashMap<String, serde_json::Value>,
    ) -> Result<Value> {
        let start_time = Instant::now();
        let Some((component_name, function_name)) = tool_name.split_once(".") else {
            return Err(WasiMcpError::InvalidArguments(format!(
                "Tool name must be in format 'component.function', got: {tool_name}",
            )));
        };

        // Get function info first
        let function_info = {
            let component = self
                .components
                .get(component_name)
                .ok_or_else(|| WasiMcpError::ComponentNotFound(component_name.to_string()))?;

            component
                .get_function_info(function_name)
                .ok_or_else(|| WasiMcpError::FunctionNotFound(function_name.to_string()))?
                .clone()
        };

        let positional_args = self.map_named_to_positional_arguments(&function_info, &arguments)?;
        let mut results = Vec::new();
        for _ in 0..function_info.results.len() {
            results.push(wasmtime::component::Val::String(String::new()));
        }

        let args = convert_args_to_wasm_values(&positional_args, &function_info)?;

        let component = self
            .components
            .get_mut(component_name)
            .ok_or_else(|| WasiMcpError::ComponentNotFound(component_name.to_string()))?;

        let Some(func) = function_info.func else {
            return Err(WasiMcpError::FunctionNotFound(function_info.name));
        };

        component.call_async(&func, &args, &mut results).await?;
        let result = if results.is_empty() {
            Value::String("Successfully executed (no return value)".to_string())
        } else {
            convert_wasm_results_to_json(&results)?
        };

        tracing::Span::current().record("duration_ms", start_time.elapsed().as_millis());
        Ok(result)
    }

    /// List all available component names
    pub fn list_components(&self) -> Vec<String> {
        self.components.keys().cloned().collect()
    }
}
