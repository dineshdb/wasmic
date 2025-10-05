use crate::error::{Result, WasiMcpError};
use serde_json::Value;
use wasmtime::component::Val;

/// Convert a serde_json::Value to a wasmtime::component::Val
#[allow(unused)]
fn to_wasm(json_value: &Value) -> Result<Val> {
    to_wasm_with_type(json_value, None)
}

/// Convert a serde_json::Value to a wasmtime::component::Val with type information
pub fn to_wasm_with_type(
    json_value: &Value,
    wasm_type: Option<&wasmtime::component::Type>,
) -> Result<Val> {
    match json_value {
        Value::Null => Ok(Val::String("null".to_string())),
        Value::Bool(b) => Ok(Val::Bool(*b)),
        Value::Number(n) => {
            // If we have WASM type information, use it to determine the correct type
            if let Some(wasm_type) = wasm_type {
                match wasm_type {
                    wasmtime::component::Type::U8 => {
                        if let Some(u) = n.as_u64() {
                            if u <= u8::MAX as u64 {
                                Ok(Val::U8(u as u8))
                            } else {
                                Err(WasiMcpError::InvalidArguments(format!(
                                    "Value {u} exceeds u8 range",
                                )))
                            }
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected unsigned integer for u8 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::U16 => {
                        if let Some(u) = n.as_u64() {
                            if u <= u16::MAX as u64 {
                                Ok(Val::U16(u as u16))
                            } else {
                                Err(WasiMcpError::InvalidArguments(format!(
                                    "Value {u} exceeds u16 range",
                                )))
                            }
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected unsigned integer for u16 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::U32 => {
                        if let Some(u) = n.as_u64() {
                            if u <= u32::MAX as u64 {
                                Ok(Val::U32(u as u32))
                            } else {
                                Err(WasiMcpError::InvalidArguments(format!(
                                    "Value {u} exceeds u32 range",
                                )))
                            }
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected unsigned integer for u32 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::U64 => {
                        if let Some(u) = n.as_u64() {
                            Ok(Val::U64(u))
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected unsigned integer for u64 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::S8 => {
                        if let Some(i) = n.as_i64() {
                            if i >= i8::MIN as i64 && i <= i8::MAX as i64 {
                                Ok(Val::S8(i as i8))
                            } else {
                                Err(WasiMcpError::InvalidArguments(format!(
                                    "Value {i} exceeds s8 range",
                                )))
                            }
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected signed integer for s8 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::S16 => {
                        if let Some(i) = n.as_i64() {
                            if i >= i16::MIN as i64 && i <= i16::MAX as i64 {
                                Ok(Val::S16(i as i16))
                            } else {
                                Err(WasiMcpError::InvalidArguments(format!(
                                    "Value {i} exceeds s16 range",
                                )))
                            }
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected signed integer for s16 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::S32 => {
                        if let Some(i) = n.as_i64() {
                            if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                                Ok(Val::S32(i as i32))
                            } else {
                                Err(WasiMcpError::InvalidArguments(format!(
                                    "Value {i} exceeds s32 range",
                                )))
                            }
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected signed integer for s32 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::S64 => {
                        if let Some(i) = n.as_i64() {
                            Ok(Val::S64(i))
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected signed integer for s64 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::Float32 => {
                        if let Some(f) = n.as_f64() {
                            Ok(Val::Float32(f as f32))
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected float for f32 type".to_string(),
                            ))
                        }
                    }
                    wasmtime::component::Type::Float64 => {
                        if let Some(f) = n.as_f64() {
                            Ok(Val::Float64(f))
                        } else {
                            Err(WasiMcpError::InvalidArguments(
                                "Expected float for f64 type".to_string(),
                            ))
                        }
                    }
                    // For other types, fall back to default behavior
                    _ => {
                        if n.is_i64() {
                            Ok(Val::S64(n.as_i64().unwrap()))
                        } else if n.is_u64() {
                            Ok(Val::U64(n.as_u64().unwrap()))
                        } else {
                            // Handle f64 values
                            Ok(Val::Float64(n.as_f64().unwrap()))
                        }
                    }
                }
            } else {
                // Default behavior when no type information is provided
                if n.is_i64() {
                    Ok(Val::S64(n.as_i64().unwrap()))
                } else if n.is_u64() {
                    Ok(Val::U64(n.as_u64().unwrap()))
                } else {
                    // Handle f64 values
                    Ok(Val::Float64(n.as_f64().unwrap()))
                }
            }
        }
        Value::String(s) => Ok(Val::String(s.clone())),
        Value::Array(arr) => {
            let wasm_values: Result<Vec<Val>> =
                arr.iter().map(|v| to_wasm_with_type(v, None)).collect();
            Ok(Val::List(wasm_values?))
        }
        Value::Object(obj) => {
            // If we have WASM type information and it's a record, use the field order from the type
            if let Some(wasmtime::component::Type::Record(record_type)) = wasm_type {
                let expected_fields: Vec<&str> = record_type.fields().map(|f| f.name).collect();
                let mut record_fields = Vec::with_capacity(expected_fields.len());

                // Create a map for quick lookup
                let obj_map: std::collections::HashMap<&str, &Value> =
                    obj.iter().map(|(k, v)| (k.as_str(), v)).collect();

                // Add fields in the expected order
                for field in record_type.fields() {
                    let field_name = field.name;
                    let field_type = field.ty.clone();
                    if let Some(field_value) = obj_map.get(field_name) {
                        let wasm_val = to_wasm_with_type(field_value, Some(&field_type))?;
                        record_fields.push((field_name.to_string(), wasm_val));
                    } else {
                        return Err(WasiMcpError::InvalidArguments(format!(
                            "Missing required field: '{field_name}'",
                        )));
                    }
                }

                // Check for extra fields that aren't in the expected record
                for field_name in obj.keys() {
                    if !expected_fields.contains(&field_name.as_str()) {
                        return Err(WasiMcpError::InvalidArguments(format!(
                            "Unexpected field: '{field_name}'",
                        )));
                    }
                }

                Ok(Val::Record(record_fields))
            } else {
                // Fallback to original behavior for non-typed objects
                let record_fields: Result<Vec<(String, Val)>> = obj
                    .iter()
                    .map(|(key, value)| {
                        to_wasm_with_type(value, None).map(|wasm_val| (key.clone(), wasm_val))
                    })
                    .collect();
                Ok(Val::Record(record_fields?))
            }
        }
    }
}

/// Convert a wasmtime::component::Val to a serde_json::Value
pub fn wasm_to_json(wasm_value: &Val) -> Result<Value> {
    match wasm_value {
        Val::Bool(b) => Ok(Value::Bool(*b)),
        Val::S8(i) => Ok(Value::Number(serde_json::Number::from(*i))),
        Val::U8(u) => Ok(Value::Number(serde_json::Number::from(*u))),
        Val::S16(i) => Ok(Value::Number(serde_json::Number::from(*i))),
        Val::U16(u) => Ok(Value::Number(serde_json::Number::from(*u))),
        Val::S32(i) => Ok(Value::Number(serde_json::Number::from(*i))),
        Val::U32(u) => Ok(Value::Number(serde_json::Number::from(*u))),
        Val::S64(i) => Ok(Value::Number(serde_json::Number::from(*i))),
        Val::U64(u) => Ok(Value::Number(serde_json::Number::from(*u))),
        Val::Float32(f) => Ok(Value::Number(
            serde_json::Number::from_f64(*f as f64).unwrap_or(serde_json::Number::from(0)),
        )),
        Val::Float64(f) => Ok(Value::Number(
            serde_json::Number::from_f64(*f).unwrap_or(serde_json::Number::from(0)),
        )),
        Val::Char(c) => Ok(Value::String(c.to_string())),
        Val::String(s) => Ok(Value::String(s.clone())),
        Val::List(vals) => {
            let json_values: Result<Vec<Value>> = vals.iter().map(wasm_to_json).collect();
            Ok(Value::Array(json_values?))
        }
        Val::Record(fields) => {
            let mut obj = serde_json::Map::new();
            for (key, val) in fields {
                obj.insert(key.clone(), wasm_to_json(val)?);
            }
            Ok(Value::Object(obj))
        }
        Val::Tuple(vals) => {
            let json_values: Result<Vec<Value>> = vals.iter().map(wasm_to_json).collect();
            Ok(Value::Array(json_values?))
        }
        Val::Variant(name, value) => {
            let mut obj = serde_json::Map::new();
            obj.insert("variant".to_string(), Value::String(name.clone()));
            if let Some(val) = value {
                obj.insert("value".to_string(), wasm_to_json(val)?);
            } else {
                obj.insert("value".to_string(), Value::Null);
            }
            Ok(Value::Object(obj))
        }
        Val::Enum(name) => Ok(Value::String(name.clone())),
        Val::Option(opt_val) => match opt_val {
            Some(val) => wasm_to_json(val),
            None => Ok(Value::Null),
        },
        Val::Result(result) => match result {
            Ok(ok_val) => {
                let mut obj = serde_json::Map::new();
                obj.insert("result".to_string(), Value::String("ok".to_string()));
                if let Some(val) = ok_val {
                    obj.insert("value".to_string(), wasm_to_json(val)?);
                } else {
                    obj.insert("value".to_string(), Value::Null);
                }
                Ok(Value::Object(obj))
            }
            Err(err_val) => {
                let mut obj = serde_json::Map::new();
                obj.insert("result".to_string(), Value::String("error".to_string()));
                if let Some(val) = err_val {
                    obj.insert("value".to_string(), wasm_to_json(val)?);
                } else {
                    obj.insert("value".to_string(), Value::Null);
                }
                Ok(Value::Object(obj))
            }
        },
        Val::Flags(flags) => {
            let flag_values: Vec<Value> = flags.iter().map(|f| Value::String(f.clone())).collect();
            Ok(Value::Array(flag_values))
        }
        Val::Resource(_) => Ok(Value::String("[Resource]".to_string())),
        Val::Future(_) => Ok(Value::String("[Future]".to_string())),
        Val::Stream(_) => Ok(Value::String("[Stream]".to_string())),
        Val::ErrorContext(_) => Ok(Value::String("[ErrorContext]".to_string())),
    }
}

/// Convert WASM result values to JSON with proper formatting
pub fn convert_wasm_results_to_json(wasm_results: &[Val]) -> Result<Value> {
    match wasm_results.len() {
        0 => Ok(Value::String(
            "Successfully executed (no return value)".to_string(),
        )),
        1 => wasm_to_json(&wasm_results[0]),
        _ => {
            let json_results: Result<Vec<Value>> = wasm_results.iter().map(wasm_to_json).collect();
            Ok(Value::Array(json_results?))
        }
    }
}

/// Convert JSON arguments to WASM values using the transformer
pub fn convert_args_to_wasm_values(
    arguments: &[serde_json::Value],
    function_info: &crate::wasm::FunctionInfo,
) -> Result<Vec<wasmtime::component::Val>> {
    let mut wasm_values = Vec::with_capacity(arguments.len());

    for (i, (arg, param_info)) in arguments.iter().zip(&function_info.params).enumerate() {
        let wasm_val = convert_json_to_wasm_value(arg, &param_info.wasm_type).map_err(|e| {
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
            to_wasm_with_type(json_value, Some(wasm_type))
        }
        wasmtime::component::Type::List(_) => {
            // Use ValueTransformer to properly convert JSON arrays to WASM lists with type information
            to_wasm_with_type(json_value, Some(wasm_type))
        }
        wasmtime::component::Type::Tuple(_) => {
            // Use ValueTransformer to properly convert JSON arrays to WASM tuples with type information
            to_wasm_with_type(json_value, Some(wasm_type))
        }
        wasmtime::component::Type::Variant(_) => {
            // Use ValueTransformer to properly convert JSON objects to WASM variants with type information
            to_wasm_with_type(json_value, Some(wasm_type))
        }
        wasmtime::component::Type::Enum(_) => {
            // Use ValueTransformer to properly convert JSON strings to WASM enums with type information
            to_wasm_with_type(json_value, Some(wasm_type))
        }
        wasmtime::component::Type::Option(_) => {
            // Use ValueTransformer to properly convert JSON values to WASM options with type information
            to_wasm_with_type(json_value, Some(wasm_type))
        }
        wasmtime::component::Type::Result(_) => {
            // Use ValueTransformer to properly convert JSON objects to WASM results with type information
            to_wasm_with_type(json_value, Some(wasm_type))
        }
        wasmtime::component::Type::Flags(_) => {
            // Use ValueTransformer to properly convert JSON arrays to WASM flags with type information
            to_wasm_with_type(json_value, Some(wasm_type))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wasmtime::component::Type;

    #[test]
    fn test_json_bool_to_wasm() {
        let json_val = Value::Bool(true);
        let wasm_val = to_wasm(&json_val).unwrap();
        assert_eq!(wasm_val, Val::Bool(true));
    }

    #[test]
    fn test_wasm_bool_to_json() {
        let wasm_val = Val::Bool(false);
        let json_val = wasm_to_json(&wasm_val).unwrap();
        assert_eq!(json_val, Value::Bool(false));
    }

    #[test]
    fn test_json_number_to_wasm() {
        let json_val = Value::Number(serde_json::Number::from(42));
        let wasm_val = to_wasm(&json_val).unwrap();
        assert_eq!(wasm_val, Val::S64(42));
    }

    #[test]
    fn test_json_string_to_wasm() {
        let json_val = Value::String("hello".to_string());
        let wasm_val = to_wasm(&json_val).unwrap();
        assert_eq!(wasm_val, Val::String("hello".to_string()));
    }

    #[test]
    fn test_json_array_to_wasm() {
        let json_val = json!([1, 2, 3]);
        let wasm_val = to_wasm(&json_val).unwrap();
        match wasm_val {
            Val::List(vals) => {
                assert_eq!(vals.len(), 3);
                assert_eq!(vals[0], Val::S64(1));
                assert_eq!(vals[1], Val::S64(2));
                assert_eq!(vals[2], Val::S64(3));
            }
            _ => panic!("Expected Val::List"),
        }
    }

    #[test]
    fn test_json_object_to_wasm() {
        let json_val = json!({"key": "value"});
        let wasm_val = to_wasm(&json_val).unwrap();
        match wasm_val {
            Val::Record(fields) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "key");
                assert_eq!(fields[0].1, Val::String("value".to_string()));
            }
            _ => panic!("Expected Val::Record"),
        }
    }

    #[test]
    fn test_wasm_list_to_json() {
        let wasm_val = Val::List(vec![
            Val::String("a".to_string()),
            Val::String("b".to_string()),
        ]);
        let json_val = wasm_to_json(&wasm_val).unwrap();
        assert_eq!(json_val, json!(["a", "b"]));
    }

    #[test]
    fn test_wasm_record_to_json() {
        let wasm_val = Val::Record(vec![
            ("name".to_string(), Val::String("test".to_string())),
            ("value".to_string(), Val::U32(42)),
        ]);
        let json_val = wasm_to_json(&wasm_val).unwrap();
        assert_eq!(json_val, json!({"name": "test", "value": 42}));
    }

    #[test]
    fn test_type_checked_conversion() {
        let json_val = Value::Number(serde_json::Number::from(100));
        let wasm_val = to_wasm_with_type(&json_val, Some(&Type::U8)).unwrap();
        assert_eq!(wasm_val, Val::U8(100));

        // Test overflow
        let json_val = Value::Number(serde_json::Number::from(300));
        let result = to_wasm_with_type(&json_val, Some(&Type::S8));
        assert!(result.is_err());
    }

    #[test]
    fn test_result_conversion() {
        let wasm_val = Val::Result(Ok(Some(Box::new(Val::String("success".to_string())))));
        let json_val = wasm_to_json(&wasm_val).unwrap();

        match json_val {
            Value::Object(obj) => {
                assert_eq!(obj.get("result"), Some(&Value::String("ok".to_string())));
                assert_eq!(
                    obj.get("value"),
                    Some(&Value::String("success".to_string()))
                );
            }
            _ => panic!("Expected object for result type"),
        }
    }
}
