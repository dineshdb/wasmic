use crate::error::Result;
use rmcp::model::Tool;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use wasmtime::{
    Engine,
    component::{Component, types::ComponentItem},
};

/// Component export information with optimized memory usage
#[derive(Debug, Clone, Default)]
pub struct ComponentExports {
    pub functions: Vec<FunctionInfo>,
    pub interfaces: Vec<InterfaceInfo>,
}

/// Interface information containing functions
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub name: String,
    pub full_name: String,
    pub functions: HashMap<String, FunctionInfo>,
}

/// Parameter information combining name, type, and position
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    pub name: String,
    pub param_type: String,
    pub position: usize,
}

/// Function information with optimized field types
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub params: Vec<ParameterInfo>, // Function parameters with position info
    pub results: Vec<String>,       // Function return types/results
}

/// Recursively extract exports from a component item with optimized processing and reduced allocations
pub fn get_exports(engine: &Engine, path: &str, item: &ComponentItem) -> ComponentExports {
    let mut exports = ComponentExports {
        functions: Vec::with_capacity(4), // Pre-allocate with reasonable capacity
        interfaces: Vec::with_capacity(1), // Most components have few interfaces
    };

    match item {
        ComponentItem::ComponentFunc(f) => {
            let results: Vec<String> = f.results().map(|t| format!("{t:?}")).collect();

            // Create parameter info with position - optimized allocation
            let params: Vec<ParameterInfo> = f
                .params()
                .enumerate()
                .map(|(position, (n, t))| ParameterInfo {
                    name: n.to_string(),
                    param_type: format!("{t:?}"),
                    position,
                })
                .collect();

            exports.functions.push(FunctionInfo {
                name: path.to_string(),
                params,
                results,
            });
        }
        ComponentItem::CoreFunc(_ft) => {
            // todo: improve param/result extraction
        }
        ComponentItem::ComponentInstance(inst) => {
            let mut interface_functions = HashMap::with_capacity(4); // Pre-allocate

            for (name, nested) in inst.exports(engine) {
                let child = format!("{path}.{name}");
                let nested_result = get_exports(engine, &child, &nested);

                // Add functions from nested inspection
                for func in nested_result.functions {
                    // Keep the original function name, but create the full path for tool execution
                    let function_key = func.name.clone(); // Original function name
                    let full_function_path = format!("{path}.{name}"); // Full path for execution

                    // Create a new function info with the proper name for execution
                    let mut func_for_interface = func.clone();
                    func_for_interface.name = full_function_path;

                    interface_functions.insert(function_key, func_for_interface);
                }

                // Add interfaces from nested inspection
                exports.interfaces.extend(nested_result.interfaces);
            }

            // Create interface info for this instance if it has functions
            if !interface_functions.is_empty() {
                let interface_parts: Vec<&str> = path.split('/').collect();
                let interface_display_name = interface_parts.last().copied().unwrap_or(path);

                let interface_info = InterfaceInfo {
                    name: interface_display_name.to_string(),
                    full_name: path.to_string(),
                    functions: interface_functions,
                };

                exports.interfaces.push(interface_info);
            }
        }
        ComponentItem::Component(nested_comp) => {
            // Nested component defined inside this component
            for (name, nested) in nested_comp.exports(engine) {
                let child = format!("{path}.{name}");
                let nested_result = get_exports(engine, &child, &nested);

                // Add all results from nested inspection
                exports.functions.extend(nested_result.functions);
                exports.interfaces.extend(nested_result.interfaces);
            }
        }
        ComponentItem::Module(_) => {
            // Module types are not currently used, skip collecting them
        }
        ComponentItem::Type(_) => {
            // Type information is not currently used, skip collecting it
        }
        ComponentItem::Resource(_) => {
            // Resource information is not currently used, skip collecting it
        }
    }

    exports
}

/// WASM component with improved caching and performance
pub struct WasmComponent {
    pub name: String,
    pub engine: Arc<Engine>,
    pub component: Component,
    pub interfaces: HashMap<String, InterfaceInfo>, // Map of interface name to interface info
    pub functions: HashMap<String, FunctionInfo>, // Map of function name to function info for standalone functions
}

impl WasmComponent {
    /// Create a new WASM component from file path with shared engine (optimized)
    pub fn new_with_engine(name: String, wasm_path: &PathBuf, engine: Arc<Engine>) -> Result<Self> {
        let start_time = std::time::Instant::now();

        // Load the component
        let component = Component::from_file(&engine, wasm_path)
            .map_err(crate::error::WasiMcpError::Component)?;

        let load_time = start_time.elapsed();
        tracing::debug!("Component file loading took: {:?}", load_time);

        // Extract component info with optimized processing
        let analysis_start = std::time::Instant::now();
        let (interfaces, functions) = Self::extract_component_info(&engine, &component)?;
        let analysis_time = analysis_start.elapsed();

        tracing::info!(
            "Loaded component: {name} with {} interfaces and {} standalone functions (analysis: {:?})",
            interfaces.len(),
            functions.len(),
            analysis_time
        );

        Ok(Self {
            name,
            engine,
            component,
            interfaces,
            functions,
        })
    }

    /// Extract component information with optimized processing and reduced logging
    fn extract_component_info(
        engine: &Engine,
        component: &Component,
    ) -> Result<(
        HashMap<String, InterfaceInfo>,
        HashMap<String, FunctionInfo>,
    )> {
        let mut interfaces = HashMap::with_capacity(4); // Pre-allocate with reasonable capacity
        let mut functions = HashMap::with_capacity(8); // Pre-allocate with reasonable capacity
        let ty = component.component_type();

        // Walk top-level exports and use get_exports to get all information
        for (name, item) in ty.exports(engine) {
            let exports = get_exports(engine, name, &item);

            // Only log at debug level to reduce overhead
            tracing::debug!(
                "Found {} functions, {} interfaces in {}",
                exports.functions.len(),
                exports.interfaces.len(),
                name
            );

            // Process standalone functions (top-level functions not in interfaces)
            for func in exports.functions {
                // Only add as standalone function if it's not part of an interface
                if !exports
                    .interfaces
                    .iter()
                    .any(|interface| interface.functions.contains_key(&func.name))
                {
                    functions.insert(func.name.clone(), func);
                }
            }

            // Process interfaces and their functions
            for interface in &exports.interfaces {
                // Add interface to our collections if it has functions
                if !interface.functions.is_empty() {
                    interfaces.insert(interface.full_name.clone(), interface.clone());
                }
            }
        }

        tracing::debug!(
            "Extracted {} interfaces and {} standalone functions",
            interfaces.len(),
            functions.len()
        );
        Ok((interfaces, functions))
    }

    /// Get all tools from the component with component description included
    pub fn get_tools(&self, component_description: Option<&str>) -> Result<Vec<Tool>> {
        let mut tools = Vec::new();
        let ty = self.component.component_type();

        // Walk top-level exports and use get_exports to get all information
        for (name, item) in ty.exports(&self.engine) {
            let exports = get_exports(&self.engine, name, &item);

            // Process top-level functions
            for func in &exports.functions {
                tools.push(Self::create_tool_from_function(
                    &func.name,
                    &func.params,
                    &func.results,
                    None, // No interface name for top-level functions
                    component_description,
                ));
            }

            // Process interfaces and their functions
            for interface in &exports.interfaces {
                for (func_name, func_info) in &interface.functions {
                    tools.push(Self::create_tool_from_function(
                        func_name,
                        &func_info.params,
                        &func_info.results,
                        Some(&interface.name),
                        component_description,
                    ));
                }
            }
        }

        tracing::debug!("Created {} tools from component", tools.len());
        Ok(tools)
    }

    /// Map WASM component types to JSON schema types
    fn map_wasm_type_to_json_type(wasm_type: &str) -> serde_json::Value {
        match wasm_type {
            // String types
            t if t.contains("String") || t.contains("string") => serde_json::json!("string"),

            // Integer types
            t if t.contains("U8") || t.contains("u8") => serde_json::json!("integer"),
            t if t.contains("U16") || t.contains("u16") => serde_json::json!("integer"),
            t if t.contains("U32") || t.contains("u32") => serde_json::json!("integer"),
            t if t.contains("U64") || t.contains("u64") => serde_json::json!("integer"),
            t if t.contains("S8") || t.contains("s8") => serde_json::json!("integer"),
            t if t.contains("S16") || t.contains("s16") => serde_json::json!("integer"),
            t if t.contains("S32") || t.contains("s32") => serde_json::json!("integer"),
            t if t.contains("S64") || t.contains("s64") => serde_json::json!("integer"),

            // Float types
            t if t.contains("F32") || t.contains("f32") => serde_json::json!("number"),
            t if t.contains("F64") || t.contains("f64") => serde_json::json!("number"),

            // Boolean types
            t if t.contains("Bool") || t.contains("bool") => serde_json::json!("boolean"),

            // List/Array types
            t if t.contains("List") || t.contains("Vec") || t.contains("[]") => {
                serde_json::json!("array")
            }

            // Option types (nullable)
            t if t.contains("Option") => {
                // Extract the inner type and make it nullable
                let inner_type = t.replace("Option<", "").replace(">", "");
                let mapped_type = Self::map_wasm_type_to_json_type(&inner_type);
                serde_json::json!({
                    "oneOf": [
                        mapped_type,
                        { "type": "null" }
                    ]
                })
            }

            // Record/Object types
            t if t.contains("Record") || t.contains("Tuple") => serde_json::json!("object"),

            // Default fallback for unknown types
            _ => serde_json::json!("string"),
        }
    }

    /// Create a tool from function information with proper JSON schema generation
    fn create_tool_from_function(
        function_name: &str,
        params: &Vec<ParameterInfo>,
        results: &[String],
        interface_name: Option<&str>,
        component_description: Option<&str>,
    ) -> Tool {
        let tool_name = function_name.to_string();

        // Build the base description
        let base_description = if let Some(iface_name) = interface_name {
            format!(
                "Function {function_name} from interface {iface_name} with params: {params:?}, results: {results:?}",
            )
        } else {
            format!("Function {function_name} with params: {params:?}, results: {results:?}",)
        };

        // Add component description if available
        let description = if let Some(comp_desc) = component_description {
            format!("{}\n\nComponent: {}", base_description, comp_desc)
        } else {
            base_description
        };

        // Create input schema based on function parameters with proper JSON schema types
        let input_schema = if params.is_empty() {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            })
        } else {
            let mut properties = serde_json::Map::with_capacity(params.len());
            let mut required = Vec::with_capacity(params.len());

            for param_info in params.iter() {
                let json_type = Self::map_wasm_type_to_json_type(&param_info.param_type);

                let mut param_schema = serde_json::Map::new();
                param_schema.insert("type".to_string(), json_type);
                param_schema.insert(
                    "description".to_string(),
                    serde_json::Value::String(format!(
                        "Parameter: {} (position: {}, WASM type: {})",
                        param_info.name, param_info.position, param_info.param_type
                    )),
                );

                // Add additional constraints for numeric types
                if param_info.param_type.contains("U8") {
                    param_schema.insert("minimum".to_string(), serde_json::json!(0));
                    param_schema.insert("maximum".to_string(), serde_json::json!(255));
                } else if param_info.param_type.contains("U16") {
                    param_schema.insert("minimum".to_string(), serde_json::json!(0));
                    param_schema.insert("maximum".to_string(), serde_json::json!(65535));
                } else if param_info.param_type.contains("U32") {
                    param_schema.insert("minimum".to_string(), serde_json::json!(0));
                    param_schema.insert(
                        "maximum".to_string(),
                        serde_json::Value::Number(serde_json::Number::from(4294967295u64)),
                    );
                } else if param_info.param_type.contains("S8") {
                    param_schema.insert("minimum".to_string(), serde_json::json!(-128));
                    param_schema.insert("maximum".to_string(), serde_json::json!(127));
                } else if param_info.param_type.contains("S16") {
                    param_schema.insert("minimum".to_string(), serde_json::json!(-32768));
                    param_schema.insert("maximum".to_string(), serde_json::json!(32767));
                } else if param_info.param_type.contains("S32") {
                    param_schema.insert("minimum".to_string(), serde_json::json!(-2147483648));
                    param_schema.insert("maximum".to_string(), serde_json::json!(2147483647));
                }

                properties.insert(
                    param_info.name.clone(),
                    serde_json::Value::Object(param_schema),
                );
                required.push(&param_info.name);
            }

            serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            })
        };

        Tool {
            name: tool_name.into(),
            title: None,
            description: Some(description.into()),
            input_schema: Arc::new(input_schema.as_object().cloned().unwrap_or_default()),
            output_schema: None,
            annotations: None,
            icons: None,
        }
    }

    /// Get function information by name
    pub fn get_function_info(&self, function_name: &str) -> Option<&FunctionInfo> {
        // First try to find in interfaces
        for interface in self.interfaces.values() {
            if let Some(func_info) = interface.functions.get(function_name) {
                return Some(func_info);
            }
        }

        // If not found in interfaces, try standalone functions
        self.functions.get(function_name)
    }
}

/// Tool call result structure
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct WasmToolResult {
    pub tool_name: String,
    pub result: String,
    pub status: String,
}
