/// Convert a wasmtime Type directly to JSON schema type
pub fn convert_wasm_type_to_json(ty: &wasmtime::component::Type) -> serde_json::Value {
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
