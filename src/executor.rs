use crate::config::ComponentConfig;
use crate::error::{Result, WasiMcpError};
use crate::linker::create_wasi_context_with_volume_mounts;
use crate::state::ComponentRunStates;
use crate::wasm::{WasmComponent, WasmToolResult};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;
use wasmtime::*;

/// Unified executor that can handle both single and multiple WASM components
pub struct WasmExecutor {
    engine: Arc<Engine>,
    components: HashMap<String, Arc<WasmComponent>>,
    profile: crate::config::Profile,
    linker: Arc<wasmtime::component::Linker<ComponentRunStates>>,
}

impl WasmExecutor {
    /// Create a new UnifiedExecutor with a shared engine and global linker
    pub fn new(engine: Arc<Engine>, profile: crate::config::Profile) -> Result<Self> {
        let mut linker = wasmtime::component::Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|e| {
            WasiMcpError::Execution(format!("Failed to add WASI preview2 imports: {e}"))
        })?;

        // Add HTTP bindings for async support
        wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker)
            .map_err(|e| WasiMcpError::Execution(format!("Failed to add HTTP imports: {e}")))?;

        Ok(Self {
            engine: engine.clone(),
            components: HashMap::new(),
            profile,
            linker: Arc::new(linker),
        })
    }

    /// Add a component to the executor
    pub fn add_component(&mut self, name: String, component: WasmComponent) -> Result<()> {
        let component_arc = Arc::new(component);
        self.components.insert(name.clone(), component_arc);

        tracing::debug!(
            "Added component '{name}' with {} tools",
            self.get_component_tools(&name)?.len()
        );
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

    /// Get tools for a specific component
    pub fn get_component_tools(&self, component_name: &str) -> Result<Vec<rmcp::model::Tool>> {
        self.components
            .get(component_name)
            .ok_or_else(|| WasiMcpError::ComponentNotFound(component_name.to_string()))
            .and_then(|component| {
                let component_config = self.get_component_config(component_name);
                let component_description =
                    component_config.and_then(|config| config.description.as_deref());
                let mut tools = component.get_tools(component_description)?;
                // Prefix tool names with component name
                for tool in &mut tools {
                    tool.name = format!("{}.{}", component_name, tool.name).into();
                }
                Ok(tools)
            })
    }

    /// Map named arguments to positional arguments based on function signature
    fn map_named_to_positional_arguments(
        &self,
        function_info: &crate::wasm::FunctionInfo,
        named_args: &HashMap<String, serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>> {
        let mut positional_args = Vec::with_capacity(function_info.params.len());
        let mut sorted_params: Vec<&crate::wasm::ParameterInfo> =
            function_info.params.iter().collect();
        sorted_params.sort_by(|a, b| a.position.cmp(&b.position));

        // Check for missing required arguments
        for param_info in &sorted_params {
            if !named_args.contains_key(&param_info.name) {
                return Err(WasiMcpError::InvalidArguments(format!(
                    "Missing required argument: '{}' (position: {})",
                    param_info.name, param_info.position
                )));
            }
        }

        // Check for extra arguments that aren't in the function signature
        for arg_name in named_args.keys() {
            if !function_info.params.iter().any(|p| p.name == *arg_name) {
                return Err(WasiMcpError::InvalidArguments(format!(
                    "Unexpected argument: '{arg_name}'"
                )));
            }
        }

        // Map arguments in the correct order based on parameter positions
        for param_info in &sorted_params {
            if let Some(arg_value) = named_args.get(&param_info.name) {
                positional_args.push(arg_value.clone());
            } else {
                // This should not happen due to the missing argument check above
                return Err(WasiMcpError::InvalidArguments(format!(
                    "Argument '{}' (position: {}) not found in provided arguments",
                    param_info.name, param_info.position
                )));
            }
        }

        tracing::debug!(
            "Mapped named arguments {:?} to positional arguments {:?}",
            named_args,
            positional_args
        );

        Ok(positional_args)
    }

    /// Execute a function from any of the managed components with named arguments
    pub async fn execute_function(
        &self,
        tool_name: &str,
        arguments: HashMap<String, serde_json::Value>,
    ) -> Result<WasmToolResult> {
        // Find the component and function name
        if let Some((component_name, function_name)) = tool_name.split_once('.') {
            if let Some(component) = self.components.get(component_name) {
                self.execute_function_in_component(component, function_name, &arguments)
                    .await
            } else {
                Err(WasiMcpError::ComponentNotFound(component_name.to_string()))
            }
        } else {
            Err(WasiMcpError::InvalidArguments(format!(
                "Tool name must be in format 'component.function', got: {tool_name}",
            )))
        }
    }

    /// Execute a function within a specific component with named arguments
    async fn execute_function_in_component(
        &self,
        component: &Arc<WasmComponent>,
        tool_name: &str,
        arguments: &HashMap<String, serde_json::Value>,
    ) -> Result<WasmToolResult> {
        let start_time = Instant::now();
        tracing::info!(
            "Executing function: {} in component {} with args: {:?}",
            tool_name,
            component.name,
            arguments
        );

        // Get function information for argument mapping
        let function_info = component
            .get_function_info(tool_name)
            .ok_or_else(|| WasiMcpError::FunctionNotFound(tool_name.to_string()))?;

        let positional_args = self.map_named_to_positional_arguments(function_info, arguments)?;
        let component_config = self.get_component_config(&component.name);
        let cwd = component_config.and_then(|config| config.cwd.as_deref());
        let volumes = component_config.map(|config| &config.volumes);

        let state = create_wasi_context_with_volume_mounts(volumes.unwrap_or(&Vec::new()), cwd)?;
        let mut store = Store::new(&self.engine, state);

        // Instantiate the component using the global linker asynchronously
        let instance = self
            .linker
            .instantiate_async(&mut store, &component.component)
            .await
            .map_err(|e| {
                WasiMcpError::Execution(format!("Failed to instantiate component: {e}"))
            })?;

        // Execute the function
        let result = self
            .execute_function_in_instance(
                &mut store,
                &instance,
                tool_name,
                &positional_args,
                &component.name,
            )
            .await;

        let execution_time = start_time.elapsed();
        tracing::debug!("Function execution completed in {:?}", execution_time);

        match result {
            Ok(result_string) => {
                let tool_result = WasmToolResult {
                    tool_name: format!("{}.{}", component.name, tool_name),
                    result: serde_json::to_string(&result_string)?,
                    status: "executed".to_string(),
                };
                tracing::info!(
                    "Successfully executed function: {}.{} in {:?}",
                    component.name,
                    tool_name,
                    execution_time
                );
                Ok(tool_result)
            }
            Err(e) => {
                tracing::error!(
                    "Failed to execute function {}.{}: {}",
                    component.name,
                    tool_name,
                    e
                );
                Err(e)
            }
        }
    }

    /// Execute a function within a specific component instance
    async fn execute_function_in_instance(
        &self,
        store: &mut Store<ComponentRunStates>,
        instance: &wasmtime::component::Instance,
        tool_name: &str,
        arguments: &Vec<serde_json::Value>,
        component_name: &str,
    ) -> Result<String> {
        tracing::info!(
            "Looking for function: {} in component {}",
            tool_name,
            component_name
        );

        // Get the component to access its interfaces and functions
        let component = self
            .components
            .get(component_name)
            .ok_or_else(|| WasiMcpError::ComponentNotFound(component_name.to_string()))?;

        // First try to find the function in interfaces
        let tool = component
            .interfaces
            .iter()
            .flat_map(|f| f.1.functions.values())
            .find(|func| func.name == tool_name)
            // If not found in interfaces, try standalone functions
            .or_else(|| component.functions.get(tool_name))
            .ok_or_else(|| {
                tracing::warn!(
                    "Function not found: {} in component {}",
                    tool_name,
                    component_name
                );
                WasiMcpError::FunctionNotFound(tool_name.to_string())
            })?;

        // Check if this is a standalone function (no dots in the name) or interface function
        let is_standalone = !tool.name.contains('.');

        if is_standalone {
            tracing::debug!(
                "Executing standalone function: {} in component {}",
                tool_name,
                component_name
            );

            // For standalone functions, get the function directly from the top level
            let func_idx = instance
                .get_export_index(&mut *store, None, tool_name)
                .ok_or_else(|| {
                    tracing::warn!(
                        "Standalone function not found: {} in component {}",
                        tool_name,
                        component_name
                    );
                    WasiMcpError::FunctionNotFound(tool_name.to_string())
                })?;

            let func = instance.get_func(&mut *store, func_idx).ok_or_else(|| {
                tracing::error!(
                    "Failed to get standalone function at index: {:?} in component {}",
                    func_idx,
                    component_name
                );
                WasiMcpError::Execution("Failed to get function reference".to_string())
            })?;

            return self
                .execute_function_call(store, func, tool_name, arguments, component_name, None)
                .await;
        }

        let (interface, function) = match tool.name.rsplit_once('.') {
            Some((interface, function)) => (interface, function),
            None => ("", tool.name.as_str()), // no dot, entire name is function
        };

        debug!("Parsed interface: {interface}, function: {function} in component {component_name}",);
        // Get interface index
        let interface_idx = instance
            .get_export_index(&mut *store, None, interface)
            .ok_or_else(|| {
                tracing::warn!(
                    "Interface not found: {} in component {}",
                    interface,
                    component_name
                );
                WasiMcpError::InterfaceNotFound(interface.to_string())
            })?;

        // Get function index
        let func_idx = instance
            .get_export_index(&mut *store, Some(&interface_idx), function)
            .ok_or_else(|| {
                tracing::warn!(
                    "Function not found: {interface}.{function} in component {}",
                    component_name
                );
                WasiMcpError::FunctionNotFound(format!("{interface}.{function}"))
            })?;

        let func = instance.get_func(&mut *store, func_idx).ok_or_else(|| {
            tracing::error!(
                "Failed to get function at index: {:?} in component {}",
                func_idx,
                component_name
            );
            WasiMcpError::Execution("Failed to get function reference".to_string())
        })?;

        // Convert Vec arguments to Vec for the function call
        self.execute_function_call(
            store,
            func,
            tool_name,
            arguments,
            component_name,
            Some(interface),
        )
        .await
    }

    /// Execute a function call with the given function reference
    async fn execute_function_call(
        &self,
        store: &mut Store<ComponentRunStates>,
        func: wasmtime::component::Func,
        tool_name: &str,
        arguments: &Vec<serde_json::Value>,
        component_name: &str,
        interface: Option<&str>,
    ) -> Result<String> {
        tracing::debug!(
            "Found function, attempting execution in component {}",
            component_name
        );

        // For WASM component functions, we need to handle results dynamically
        // Based on the function info from the component, determine if it expects results
        let mut results = Vec::new();
        tracing::debug!("Executing function with {} arguments", arguments.len());

        // Convert Vec<serde_json::Value> to Vec<wasmtime::component::Val> for wasmtime
        let wasmtime_args: Vec<wasmtime::component::Val> = arguments
            .iter()
            .map(|val| {
                // Convert serde_json::Value to wasmtime::component::Val
                match val {
                    serde_json::Value::String(s) => wasmtime::component::Val::String(s.clone()),
                    serde_json::Value::Number(n) => {
                        if n.is_i64() {
                            wasmtime::component::Val::S64(n.as_i64().unwrap())
                        } else if n.is_u64() {
                            wasmtime::component::Val::U64(n.as_u64().unwrap())
                        } else {
                            wasmtime::component::Val::S64(n.as_f64().unwrap() as i64)
                        }
                    }
                    serde_json::Value::Bool(b) => wasmtime::component::Val::Bool(*b),
                    serde_json::Value::Null => wasmtime::component::Val::String("null".to_string()),
                    _ => wasmtime::component::Val::String(val.to_string()),
                }
            })
            .collect();

        let component = self
            .components
            .get(component_name)
            .ok_or_else(|| WasiMcpError::ComponentNotFound(component_name.to_string()))?;

        // Get function info to see if it expects results
        let function_info = component.get_function_info(tool_name);
        let expects_results = function_info
            .map(|info| !info.results.is_empty())
            .unwrap_or(false);

        tracing::debug!("Function expects results: {}", expects_results);

        if expects_results {
            // Function expects results, provide a result slot
            results.push(wasmtime::component::Val::String(String::new()));
        }

        match func
            .call_async(&mut *store, &wasmtime_args, &mut results)
            .await
        {
            Ok(_) => {
                let result = if !results.is_empty() {
                    // Format all results
                    let result_strings: Vec<String> =
                        results.iter().map(|val| format!("{val:?}")).collect();
                    result_strings.join(", ")
                } else {
                    let interface_info = interface
                        .map(|i| format!(" (using interface: {i})"))
                        .unwrap_or_default();
                    format!(
                        "Successfully executed {tool_name} with args: {arguments:?} (no return value){interface_info} in component {component_name}",
                    )
                };
                tracing::debug!(
                    "Basic function execution successful in component {} with {} results",
                    component_name,
                    results.len()
                );
                Ok(result)
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("expected 1 result(s), got 0") && !expects_results {
                    // We thought it didn't expect results, but it does, try again with results
                    tracing::debug!(
                        "Retrying with results for function in component {}: {}",
                        component_name,
                        e
                    );

                    let mut retry_results = vec![wasmtime::component::Val::String(String::new())];
                    match func
                        .call_async(store, &wasmtime_args, &mut retry_results)
                        .await
                    {
                        Ok(_) => {
                            let result_strings: Vec<String> =
                                retry_results.iter().map(|val| format!("{val:?}")).collect();
                            let result = result_strings.join(", ");
                            tracing::debug!(
                                "Retry successful in component {} with {} results",
                                component_name,
                                retry_results.len()
                            );
                            Ok(result)
                        }
                        Err(retry_e) => {
                            tracing::error!(
                                "Retry also failed in component {}: {}",
                                component_name,
                                retry_e
                            );
                            Err(WasiMcpError::Execution(format!(
                                "Failed to execute function: {retry_e}"
                            )))
                        }
                    }
                } else {
                    tracing::error!(
                        "Basic function execution failed in component {}: {}",
                        component_name,
                        e
                    );
                    Err(WasiMcpError::Execution(format!(
                        "Failed to execute function: {e}"
                    )))
                }
            }
        }
    }

    /// List all available component names
    pub fn list_components(&self) -> Vec<String> {
        self.components.keys().cloned().collect()
    }
}
