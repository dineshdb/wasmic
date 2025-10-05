use crate::config::ComponentConfig;
use crate::error::{Result, WasiMcpError};
use crate::state::ComponentRunStates;
use crate::utils::transform::{convert_args_to_wasm_values, convert_wasm_results_to_json};
use crate::wasm::{WasmComponent, WasmContext};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;
use wasmtime::*;

/// Unified executor that can handle both single and multiple WASM components
pub struct WasmExecutor {
    context: Arc<WasmContext>,
    components: HashMap<String, WasmComponent>,
    profile: crate::config::Profile,
}
impl WasmExecutor {
    /// Create a new UnifiedExecutor with a shared engine and global linker
    pub fn new(context: Arc<WasmContext>, profile: crate::config::Profile) -> Result<Self> {
        Ok(Self {
            context,
            components: HashMap::new(),
            profile,
        })
    }

    /// Add a component to the executor
    #[instrument(level = "debug", skip(self, component), fields(name, tools))]
    pub async fn add_component(&mut self, name: String, component: WasmComponent) -> Result<()> {
        tracing::Span::current().record("components", component.get_tools(None)?.len());
        self.components.insert(name.clone(), component);
        Ok(())
    }

    /// Get component configuration for a specific component
    fn get_component_config(&self, component_name: &str) -> Option<&ComponentConfig> {
        self.profile.components.get(component_name)
    }

    /// Get all tools from all components
    pub fn get_all_tools(&self) -> Result<Vec<rmcp::model::Tool>> {
        let mut all_tools = Vec::new();

        for (component_name, component) in &self.components {
            let component_config = self.get_component_config(component_name);
            let component_description =
                component_config.and_then(|config| config.description.as_deref());
            let mut tools = component.get_tools(component_description)?;

            // Prefix tool names with component name to avoid conflicts
            for tool in &mut tools {
                tool.name = format!("{}.{}", component_name, tool.name).into();
            }

            all_tools.extend(tools);
        }

        Ok(all_tools)
    }

    /// Map named arguments to positional arguments based on function signature
    fn map_named_to_positional_arguments(
        &self,
        function_info: &crate::wasm::FunctionInfo,
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

    /// Create a WASI context and instantiate a component
    async fn create_component_instance(
        &self,
        component: &WasmComponent,
    ) -> Result<(Store<ComponentRunStates>, wasmtime::component::Instance)> {
        let state = ComponentRunStates::try_from(&component.config)?;
        let mut store = Store::new(&self.context.engine, state);

        // Instantiate the component using the global linker asynchronously
        let instance = self
            .context
            .linker
            .instantiate_async(&mut store, &component.component)
            .await?;

        Ok((store, instance))
    }

    pub fn get_function_from_instance(
        store: &mut Store<ComponentRunStates>,
        instance: &wasmtime::component::Instance,
        func_name: &str,
    ) -> Result<wasmtime::component::Func> {
        // Check if this is a standalone function (no dots in the name) or interface function
        let is_standalone = !func_name.contains('.');

        if is_standalone {
            // For standalone functions, get the function directly from the top level
            let func_idx = instance
                .get_export_index(&mut *store, None, func_name)
                .ok_or_else(|| WasiMcpError::FunctionNotFound(func_name.to_string()))?;

            instance.get_func(&mut *store, func_idx).ok_or_else(|| {
                WasiMcpError::Execution("Failed to get function reference".to_string())
            })
        } else {
            // For interface functions, parse the interface and function names
            let (interface, function) = match func_name.rsplit_once('.') {
                Some((interface, function)) => (interface, function),
                None => {
                    return Err(WasiMcpError::Execution(format!(
                        "Invalid function name format: {func_name}",
                    )));
                }
            };

            // Get interface index
            let interface_idx = instance
                .get_export_index(&mut *store, None, interface)
                .ok_or_else(|| WasiMcpError::InterfaceNotFound(interface.to_string()))?;

            // Get function index
            let func_idx = instance
                .get_export_index(&mut *store, Some(&interface_idx), function)
                .ok_or_else(|| WasiMcpError::FunctionNotFound(format!("{interface}.{function}")))?;

            instance.get_func(&mut *store, func_idx).ok_or_else(|| {
                WasiMcpError::Execution("Failed to get function reference".to_string())
            })
        }
    }

    /// Execute a function call with proper error handling and result processing
    async fn execute_function_call(
        &self,
        store: &mut Store<ComponentRunStates>,
        func: wasmtime::component::Func,
        arguments: &[serde_json::Value],
        function_info: &crate::wasm::FunctionInfo,
    ) -> Result<serde_json::Value> {
        let mut results = Vec::new();
        for _ in 0..function_info.results.len() {
            results.push(wasmtime::component::Val::String(String::new()));
        }

        let args = convert_args_to_wasm_values(arguments, function_info)?;
        func.call_async(&mut *store, &args, &mut results).await?;
        if results.is_empty() {
            return Ok(Value::String(
                "Successfully executed (no return value)".to_string(),
            ));
        }

        convert_wasm_results_to_json(&results)
    }

    /// Execute a function from any of the managed components with named arguments
    #[instrument(level = "debug", skip(self), fields(tool_name, arguments, duration_ms))]
    pub async fn execute_function(
        &self,
        tool_name: &str,
        arguments: HashMap<String, serde_json::Value>,
    ) -> Result<Value> {
        let start_time = Instant::now();
        let Some((component_name, function_name)) = tool_name.split_once(".") else {
            return Err(WasiMcpError::InvalidArguments(format!(
                "Tool name must be in format 'component.function', got: {tool_name}",
            )));
        };

        let component = self
            .components
            .get(component_name)
            .ok_or_else(|| WasiMcpError::ComponentNotFound(component_name.to_string()))?;

        let function_info = component
            .get_function_info(function_name)
            .ok_or_else(|| WasiMcpError::FunctionNotFound(function_name.to_string()))?;

        // Create component instance
        let (mut store, instance) = self.create_component_instance(component).await?;
        let func = Self::get_function_from_instance(&mut store, &instance, &function_info.name)?;
        let positional_args = self.map_named_to_positional_arguments(function_info, &arguments)?;
        let result = self
            .execute_function_call(&mut store, func, &positional_args, function_info)
            .await?;
        tracing::Span::current().record("duration_ms", start_time.elapsed().as_millis());
        Ok(result)
    }

    /// List all available component names
    pub fn list_components(&self) -> Vec<String> {
        self.components.keys().cloned().collect()
    }
}
