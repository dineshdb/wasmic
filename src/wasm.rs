use crate::error::Result;
use rmcp::model::Tool;
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tracing::instrument;
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
    pub param_json: serde_json::Value, // JSON schema for the type
    pub wasm_type: wasmtime::component::Type, // WASM component type for conversion
    pub position: usize,
}

/// Function information with optimized field types
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub params: Vec<ParameterInfo>, // Function parameters with position info
    pub results: Vec<serde_json::Value>, // Function return types/results as JSON
}

/// Convert a wasmtime Type directly to JSON schema type
fn convert_wasm_type_to_json(ty: &wasmtime::component::Type) -> serde_json::Value {
    match ty {
        wasmtime::component::Type::Bool => serde_json::json!("boolean"),
        wasmtime::component::Type::Char | wasmtime::component::Type::String => {
            serde_json::json!("string")
        }
        wasmtime::component::Type::S8
        | wasmtime::component::Type::U8
        | wasmtime::component::Type::S16
        | wasmtime::component::Type::U16
        | wasmtime::component::Type::S32
        | wasmtime::component::Type::U32
        | wasmtime::component::Type::S64
        | wasmtime::component::Type::U64 => serde_json::json!("integer"),
        wasmtime::component::Type::Float32 | wasmtime::component::Type::Float64 => {
            serde_json::json!("number")
        }
        wasmtime::component::Type::List(list) => {
            let element_type = convert_wasm_type_to_json(&list.ty());
            serde_json::json!({
                "type": "array",
                "items": element_type
            })
        }
        wasmtime::component::Type::Record(record) => {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();

            for field in record.fields() {
                let field_type = convert_wasm_type_to_json(&field.ty);
                properties.insert(field.name.to_string(), field_type);
                required.push(field.name);
            }

            serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            })
        }
        wasmtime::component::Type::Tuple(tuple) => {
            let items: Vec<serde_json::Value> = tuple
                .types()
                .map(|t| convert_wasm_type_to_json(&t))
                .collect();
            serde_json::json!({
                "type": "array",
                "items": items,
                "minItems": items.len(),
                "maxItems": items.len()
            })
        }
        wasmtime::component::Type::Variant(variant) => {
            let cases: Vec<serde_json::Value> = variant
                .cases()
                .map(|case| {
                    if let Some(ty) = case.ty {
                        serde_json::json!({
                            "type": "object",
                            "properties": {
                                case.name: convert_wasm_type_to_json(&ty)
                            },
                            "required": [case.name],
                            "additionalProperties": false
                        })
                    } else {
                        serde_json::json!({
                            "const": case.name
                        })
                    }
                })
                .collect();

            serde_json::json!({
                "oneOf": cases
            })
        }
        wasmtime::component::Type::Enum(enum_ty) => {
            let names: Vec<&str> = enum_ty.names().collect();
            serde_json::json!({
                "type": "string",
                "enum": names
            })
        }
        wasmtime::component::Type::Option(option) => {
            let inner_type = convert_wasm_type_to_json(&option.ty());
            serde_json::json!({
                "oneOf": [
                    inner_type,
                    { "type": "null" }
                ]
            })
        }
        wasmtime::component::Type::Result(result) => {
            let ok_type = result.ok().map(|t| convert_wasm_type_to_json(&t));
            let err_type = result.err().map(|t| convert_wasm_type_to_json(&t));

            match (ok_type, err_type) {
                (Some(ok), Some(err)) => {
                    serde_json::json!({
                        "oneOf": [
                            { "type": "object", "properties": { "Ok": ok }, "required": ["Ok"] },
                            { "type": "object", "properties": { "Err": err }, "required": ["Err"] }
                        ]
                    })
                }
                (Some(ok), None) => {
                    serde_json::json!({
                        "oneOf": [
                            { "type": "object", "properties": { "Ok": ok }, "required": ["Ok"] },
                            { "type": "null" }
                        ]
                    })
                }
                (None, Some(err)) => {
                    serde_json::json!({
                        "oneOf": [
                            { "type": "null" },
                            { "type": "object", "properties": { "Err": err }, "required": ["Err"] }
                        ]
                    })
                }
                (None, None) => {
                    serde_json::json!({
                        "oneOf": [
                            { "type": "null" },
                            { "type": "string" }
                        ]
                    })
                }
            }
        }
        wasmtime::component::Type::Flags(flags) => {
            let names: Vec<&str> = flags.names().collect();
            serde_json::json!({
                "type": "array",
                "items": {
                    "type": "string",
                    "enum": names
                },
                "uniqueItems": true
            })
        }
        wasmtime::component::Type::Own(_resource) => serde_json::json!("string"),
        wasmtime::component::Type::Borrow(_resource) => serde_json::json!("string"),
        wasmtime::component::Type::Future(future) => {
            if let Some(ty) = future.ty() {
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pending": { "type": "boolean" },
                        "value": convert_wasm_type_to_json(&ty)
                    }
                })
            } else {
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pending": { "type": "boolean" }
                    }
                })
            }
        }
        wasmtime::component::Type::Stream(stream) => {
            if let Some(ty) = stream.ty() {
                serde_json::json!({
                    "type": "array",
                    "items": convert_wasm_type_to_json(&ty)
                })
            } else {
                serde_json::json!({
                    "type": "array",
                    "items": { "type": "string" }
                })
            }
        }
        wasmtime::component::Type::ErrorContext => serde_json::json!("string"),
    }
}

/// Recursively extract exports from a component item with optimized processing and reduced allocations
pub fn get_exports(engine: &Engine, path: &str, item: &ComponentItem) -> ComponentExports {
    let mut exports = ComponentExports {
        functions: Vec::with_capacity(4), // Pre-allocate with reasonable capacity
        interfaces: Vec::with_capacity(1), // Most components have few interfaces
    };

    match item {
        ComponentItem::ComponentFunc(f) => {
            let results: Vec<serde_json::Value> =
                f.results().map(|t| convert_wasm_type_to_json(&t)).collect();

            // Create parameter info with position - optimized allocation
            let params: Vec<ParameterInfo> = f
                .params()
                .enumerate()
                .map(|(position, (n, t))| {
                    let param_json = convert_wasm_type_to_json(&t);
                    ParameterInfo {
                        name: n.to_string(),
                        param_json,
                        wasm_type: t.clone(),
                        position,
                    }
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
    #[instrument(level = "debug", skip(engine, wasm_path), fields(name, duration_ms))]
    pub fn new_with_engine(name: String, wasm_path: &PathBuf, engine: Arc<Engine>) -> Result<Self> {
        let start_time = std::time::Instant::now();
        let component = Component::from_file(&engine, wasm_path)
            .map_err(crate::error::WasiMcpError::Component)?;
        tracing::Span::current().record("duration_ms", start_time.elapsed().as_micros());
        // Extract component info with optimized processing
        let (interfaces, functions) = Self::extract_component_info(&engine, &component)?;
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
                        component_description,
                    ));
                }
            }
        }

        Ok(tools)
    }

    /// Create a tool from function information with proper JSON schema generation
    fn create_tool_from_function(
        function_name: &str,
        params: &[ParameterInfo],
        results: &[serde_json::Value],
        component_description: Option<&str>,
    ) -> Tool {
        let tool_name = function_name.to_string();
        let description = component_description.unwrap_or_default().to_string();

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
                let mut param_schema = serde_json::Map::new();

                // Use the JSON schema directly from param_json
                if let Some(obj) = param_info.param_json.as_object() {
                    param_schema.extend(obj.clone());
                } else {
                    // Fallback if it's not an object
                    param_schema.insert("type".to_string(), param_info.param_json.clone());
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

        let output_schema = if results.is_empty() {
            // Functions with no return value might still produce a success message
            serde_json::json!({
                "type": "string",
                "description": "Execution status message"
            })
        } else {
            // Multiple return values are returned as an object with positional keys
            let mut properties = serde_json::Map::with_capacity(results.len());
            for (i, result_type) in results.iter().enumerate() {
                properties.insert(format!("result_{}", i + 1), result_type.clone());
            }
            serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": properties.keys().collect::<Vec<_>>(),
                "additionalProperties": false
            })
        };

        let mut properties = serde_json::Map::with_capacity(results.len());
        for (i, result_type) in results.iter().enumerate() {
            properties.insert(format!("result_{}", i + 1), result_type.clone());
        }
        Tool {
            name: tool_name.into(),
            title: None,
            description: Some(description.into()),
            input_schema: Arc::new(input_schema.as_object().cloned().unwrap_or_default()),
            output_schema: Some(Arc::new(
                output_schema.as_object().cloned().unwrap_or_default(),
            )),
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
    pub result: Value,
    pub status: String,
}
