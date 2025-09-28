use crate::config::ComponentConfig;
use crate::error::{Result, WasiMcpError};
use crate::linker::create_wasi_context;
use crate::state::ComponentRunStates;
use crate::transform::JsonWasmTransformer;
use crate::wasm::WasmComponent;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;
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
    #[instrument(level = "debug", skip(self, component), fields(name, tools))]
    pub fn add_component(&mut self, name: String, component: WasmComponent) -> Result<()> {
        tracing::Span::current().record("components", component.get_tools(None)?.len());
        let component_arc = Arc::new(component);
        self.components.insert(name.clone(), component_arc);
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
        component: &Arc<WasmComponent>,
    ) -> Result<(Store<ComponentRunStates>, wasmtime::component::Instance)> {
        let component_config = self.get_component_config(&component.name).ok_or_else(|| {
            WasiMcpError::InvalidArguments(format!(
                "Component '{}' not found in profile",
                component.name
            ))
        })?;
        let state = create_wasi_context(component_config)?;
        let mut store = Store::new(&self.engine, state);

        // Instantiate the component using the global linker asynchronously
        let instance = self
            .linker
            .instantiate_async(&mut store, &component.component)
            .await
            .map_err(|e| {
                WasiMcpError::Execution(format!("Failed to instantiate component: {e}"))
            })?;

        Ok((store, instance))
    }

    /// Get function reference from instance using function info
    fn get_function_from_instance(
        &self,
        store: &mut Store<ComponentRunStates>,
        instance: &wasmtime::component::Instance,
        function_info: &crate::wasm::FunctionInfo,
    ) -> Result<wasmtime::component::Func> {
        // Check if this is a standalone function (no dots in the name) or interface function
        let is_standalone = !function_info.name.contains('.');

        if is_standalone {
            // For standalone functions, get the function directly from the top level
            let func_idx = instance
                .get_export_index(&mut *store, None, &function_info.name)
                .ok_or_else(|| WasiMcpError::FunctionNotFound(function_info.name.clone()))?;

            instance.get_func(&mut *store, func_idx).ok_or_else(|| {
                WasiMcpError::Execution("Failed to get function reference".to_string())
            })
        } else {
            // For interface functions, parse the interface and function names
            let (interface, function) = match function_info.name.rsplit_once('.') {
                Some((interface, function)) => (interface, function),
                None => {
                    return Err(WasiMcpError::Execution(format!(
                        "Invalid function name format: {}",
                        function_info.name
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

    /// Convert JSON arguments to WASM values using the transformer
    fn convert_args_to_wasm_values(
        &self,
        arguments: &[serde_json::Value],
        function_info: &crate::wasm::FunctionInfo,
    ) -> Result<Vec<wasmtime::component::Val>> {
        let mut wasm_values = Vec::with_capacity(arguments.len());

        for (i, (arg, param_info)) in arguments.iter().zip(&function_info.params).enumerate() {
            let wasm_val = self
                .convert_json_to_wasm_value(arg, &param_info.wasm_type)
                .map_err(|e| {
                    WasiMcpError::InvalidArguments(format!(
                        "Failed to convert argument '{}' at position {}: {}",
                        param_info.name, i, e
                    ))
                })?;
            wasm_values.push(wasm_val);
        }
        Ok(wasm_values)
    }

    /// Convert a single JSON value to WASM value based on WASM type
    fn convert_json_to_wasm_value(
        &self,
        json_value: &serde_json::Value,
        wasm_type: &wasmtime::component::Type,
    ) -> Result<wasmtime::component::Val> {
        match wasm_type {
            wasmtime::component::Type::Bool => {
                if let Some(b) = json_value.as_bool() {
                    Ok(wasmtime::component::Val::Bool(b))
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected boolean, got: {json_value}",
                    )))
                }
            }
            wasmtime::component::Type::Char | wasmtime::component::Type::String => {
                if let Some(s) = json_value.as_str() {
                    Ok(wasmtime::component::Val::String(s.to_string()))
                } else {
                    Err(WasiMcpError::UnexpectedExpected(
                        "string".to_string(),
                        json_value.to_string(),
                    ))
                }
            }
            wasmtime::component::Type::S8 => {
                if let Some(n) = json_value.as_i64() {
                    if (-128..=127).contains(&n) {
                        Ok(wasmtime::component::Val::S8(n as i8))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Expected s8 (-128-127), got: {}",
                            n
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected s8, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::U8 => {
                if let Some(n) = json_value.as_u64() {
                    if n <= 255 {
                        Ok(wasmtime::component::Val::U8(n as u8))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Expected u8 (0-255), got: {}",
                            n
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected u8, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::S16 => {
                if let Some(n) = json_value.as_i64() {
                    if (-32768..=32767).contains(&n) {
                        Ok(wasmtime::component::Val::S16(n as i16))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Expected s16 (-32768-32767), got: {}",
                            n
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected s16, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::U16 => {
                if let Some(n) = json_value.as_u64() {
                    if n <= 65535 {
                        Ok(wasmtime::component::Val::U16(n as u16))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Expected u16 (0-65535), got: {}",
                            n
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected u16, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::S32 => {
                if let Some(n) = json_value.as_i64() {
                    if (-2147483648..=2147483647).contains(&n) {
                        Ok(wasmtime::component::Val::S32(n as i32))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Expected s32 (-2147483648-2147483647), got: {}",
                            n
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected s32, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::U32 => {
                if let Some(n) = json_value.as_u64() {
                    if n <= 4294967295 {
                        Ok(wasmtime::component::Val::U32(n as u32))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Expected u32 (0-4294967295), got: {}",
                            n
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected u32, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::S64 => {
                if let Some(n) = json_value.as_i64() {
                    Ok(wasmtime::component::Val::S64(n))
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected s64, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::U64 => {
                if let Some(n) = json_value.as_u64() {
                    Ok(wasmtime::component::Val::U64(n))
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected u64, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::Float32 => {
                if let Some(n) = json_value.as_f64() {
                    Ok(wasmtime::component::Val::Float32(n as f32))
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected f32, got: {}",
                        json_value
                    )))
                }
            }
            wasmtime::component::Type::Float64 => {
                if let Some(n) = json_value.as_f64() {
                    Ok(wasmtime::component::Val::Float64(n))
                } else {
                    Err(WasiMcpError::InvalidArguments(format!(
                        "Expected f64, got: {}",
                        json_value
                    )))
                }
            }
            // Handle complex types properly
            wasmtime::component::Type::Record(_) => {
                // Use ValueTransformer to properly convert JSON objects to WASM records with type information
                JsonWasmTransformer::to_wasm_with_type(json_value, Some(wasm_type))
            }
            wasmtime::component::Type::List(_) => {
                // Use ValueTransformer to properly convert JSON arrays to WASM lists with type information
                JsonWasmTransformer::to_wasm_with_type(json_value, Some(wasm_type))
            }
            wasmtime::component::Type::Tuple(_) => {
                // Use ValueTransformer to properly convert JSON arrays to WASM tuples with type information
                JsonWasmTransformer::to_wasm_with_type(json_value, Some(wasm_type))
            }
            wasmtime::component::Type::Variant(_) => {
                // Use ValueTransformer to properly convert JSON objects to WASM variants with type information
                JsonWasmTransformer::to_wasm_with_type(json_value, Some(wasm_type))
            }
            wasmtime::component::Type::Enum(_) => {
                // Use ValueTransformer to properly convert JSON strings to WASM enums with type information
                JsonWasmTransformer::to_wasm_with_type(json_value, Some(wasm_type))
            }
            wasmtime::component::Type::Option(_) => {
                // Use ValueTransformer to properly convert JSON values to WASM options with type information
                JsonWasmTransformer::to_wasm_with_type(json_value, Some(wasm_type))
            }
            wasmtime::component::Type::Result(_) => {
                // Use ValueTransformer to properly convert JSON objects to WASM results with type information
                JsonWasmTransformer::to_wasm_with_type(json_value, Some(wasm_type))
            }
            wasmtime::component::Type::Flags(_) => {
                // Use ValueTransformer to properly convert JSON arrays to WASM flags with type information
                JsonWasmTransformer::to_wasm_with_type(json_value, Some(wasm_type))
            }
            // For remaining complex types, convert to string representation for now
            wasmtime::component::Type::Own(_)
            | wasmtime::component::Type::Borrow(_)
            | wasmtime::component::Type::Future(_)
            | wasmtime::component::Type::Stream(_)
            | wasmtime::component::Type::ErrorContext => {
                Ok(wasmtime::component::Val::String(json_value.to_string()))
            }
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

        let args = self.convert_args_to_wasm_values(arguments, function_info)?;
        func.call_async(&mut *store, &args, &mut results).await?;
        if results.is_empty() {
            return Ok(Value::String(
                "Successfully executed (no return value)".to_string(),
            ));
        }

        JsonWasmTransformer::convert_wasm_results_to_json(&results)
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
        let func = self.get_function_from_instance(&mut store, &instance, function_info)?;
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
