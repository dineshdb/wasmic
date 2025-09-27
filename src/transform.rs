use crate::error::{Result, WasiMcpError};
use serde_json::Value;
use wasmtime::component::Val;

/// Transformer for converting between serde_json::Value and wasmtime::component::Val
pub struct ValueTransformer;

impl ValueTransformer {
    /// Convert a serde_json::Value to a wasmtime::component::Val
    pub fn json_to_wasm(json_value: &Value) -> Result<Val> {
        match json_value {
            Value::Null => Ok(Val::String("null".to_string())),
            Value::Bool(b) => Ok(Val::Bool(*b)),
            Value::Number(n) => {
                if n.is_i64() {
                    Ok(Val::S64(n.as_i64().unwrap()))
                } else if n.is_u64() {
                    Ok(Val::U64(n.as_u64().unwrap()))
                } else {
                    // Handle f64 values
                    Ok(Val::Float64(n.as_f64().unwrap()))
                }
            }
            Value::String(s) => Ok(Val::String(s.clone())),
            Value::Array(arr) => {
                let wasm_values: Result<Vec<Val>> = arr.iter().map(Self::json_to_wasm).collect();
                Ok(Val::List(wasm_values?))
            }
            Value::Object(obj) => {
                let record_fields: Result<Vec<(String, Val)>> = obj
                    .iter()
                    .map(|(key, value)| {
                        Self::json_to_wasm(value).map(|wasm_val| (key.clone(), wasm_val))
                    })
                    .collect();
                Ok(Val::Record(record_fields?))
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
                let json_values: Result<Vec<Value>> = vals.iter().map(Self::wasm_to_json).collect();
                Ok(Value::Array(json_values?))
            }
            Val::Record(fields) => {
                let mut obj = serde_json::Map::new();
                for (key, val) in fields {
                    obj.insert(key.clone(), Self::wasm_to_json(val)?);
                }
                Ok(Value::Object(obj))
            }
            Val::Tuple(vals) => {
                let json_values: Result<Vec<Value>> = vals.iter().map(Self::wasm_to_json).collect();
                Ok(Value::Array(json_values?))
            }
            Val::Variant(name, value) => {
                let mut obj = serde_json::Map::new();
                obj.insert("variant".to_string(), Value::String(name.clone()));
                if let Some(val) = value {
                    obj.insert("value".to_string(), Self::wasm_to_json(val)?);
                } else {
                    obj.insert("value".to_string(), Value::Null);
                }
                Ok(Value::Object(obj))
            }
            Val::Enum(name) => Ok(Value::String(name.clone())),
            Val::Option(opt_val) => match opt_val {
                Some(val) => Self::wasm_to_json(val),
                None => Ok(Value::Null),
            },
            Val::Result(result) => match result {
                Ok(ok_val) => {
                    let mut obj = serde_json::Map::new();
                    obj.insert("result".to_string(), Value::String("ok".to_string()));
                    if let Some(val) = ok_val {
                        obj.insert("value".to_string(), Self::wasm_to_json(val)?);
                    } else {
                        obj.insert("value".to_string(), Value::Null);
                    }
                    Ok(Value::Object(obj))
                }
                Err(err_val) => {
                    let mut obj = serde_json::Map::new();
                    obj.insert("result".to_string(), Value::String("error".to_string()));
                    if let Some(val) = err_val {
                        obj.insert("value".to_string(), Self::wasm_to_json(val)?);
                    } else {
                        obj.insert("value".to_string(), Value::Null);
                    }
                    Ok(Value::Object(obj))
                }
            },
            Val::Flags(flags) => {
                let flag_values: Vec<Value> =
                    flags.iter().map(|f| Value::String(f.clone())).collect();
                Ok(Value::Array(flag_values))
            }
            Val::Resource(_) => Ok(Value::String("[Resource]".to_string())),
            Val::Future(_) => Ok(Value::String("[Future]".to_string())),
            Val::Stream(_) => Ok(Value::String("[Stream]".to_string())),
            Val::ErrorContext(_) => Ok(Value::String("[ErrorContext]".to_string())),
        }
    }

    /// Convert multiple serde_json::Value to wasmtime::component::Val
    pub fn json_vec_to_wasm_vec(json_values: &[Value]) -> Result<Vec<Val>> {
        json_values.iter().map(Self::json_to_wasm).collect()
    }

    /// Convert multiple wasmtime::component::Val to serde_json::Value
    pub fn wasm_vec_to_json_vec(wasm_values: &[Val]) -> Result<Vec<Value>> {
        wasm_values.iter().map(Self::wasm_to_json).collect()
    }

    /// Convert JSON arguments to WASM values with type checking
    pub fn convert_args_to_wasm(
        arguments: &[Value],
        expected_types: &[String],
    ) -> Result<Vec<Val>> {
        if arguments.len() != expected_types.len() {
            return Err(WasiMcpError::InvalidArguments(format!(
                "Expected {} arguments, got {}",
                expected_types.len(),
                arguments.len()
            )));
        }

        let mut wasm_values = Vec::with_capacity(arguments.len());

        for (arg, expected_type) in arguments.iter().zip(expected_types.iter()) {
            let wasm_val = Self::json_to_wasm_with_type(arg, expected_type)?;
            wasm_values.push(wasm_val);
        }

        Ok(wasm_values)
    }

    /// Convert a single JSON value to WASM value with type checking
    fn json_to_wasm_with_type(json_value: &Value, expected_type: &str) -> Result<Val> {
        match (json_value, expected_type) {
            (Value::Bool(_), t) if t.contains("Bool") || t.contains("bool") => {
                Self::json_to_wasm(json_value)
            }
            (Value::Number(n), t) if t.contains("U8") || t.contains("u8") => {
                if let Some(u) = n.as_u64() {
                    if u <= u8::MAX as u64 {
                        Ok(Val::U8(u as u8))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Value {} exceeds u8 range",
                            u
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected unsigned integer for u8 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("U16") || t.contains("u16") => {
                if let Some(u) = n.as_u64() {
                    if u <= u16::MAX as u64 {
                        Ok(Val::U16(u as u16))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Value {} exceeds u16 range",
                            u
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected unsigned integer for u16 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("U32") || t.contains("u32") => {
                if let Some(u) = n.as_u64() {
                    if u <= u32::MAX as u64 {
                        Ok(Val::U32(u as u32))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Value {} exceeds u32 range",
                            u
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected unsigned integer for u32 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("U64") || t.contains("u64") => {
                if let Some(u) = n.as_u64() {
                    Ok(Val::U64(u))
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected unsigned integer for u64 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("S8") || t.contains("s8") => {
                if let Some(i) = n.as_i64() {
                    if i >= i8::MIN as i64 && i <= i8::MAX as i64 {
                        Ok(Val::S8(i as i8))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Value {} exceeds s8 range",
                            i
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected signed integer for s8 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("S16") || t.contains("s16") => {
                if let Some(i) = n.as_i64() {
                    if i >= i16::MIN as i64 && i <= i16::MAX as i64 {
                        Ok(Val::S16(i as i16))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Value {} exceeds s16 range",
                            i
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected signed integer for s16 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("S32") || t.contains("s32") => {
                if let Some(i) = n.as_i64() {
                    if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                        Ok(Val::S32(i as i32))
                    } else {
                        Err(WasiMcpError::InvalidArguments(format!(
                            "Value {} exceeds s32 range",
                            i
                        )))
                    }
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected signed integer for s32 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("S64") || t.contains("s64") => {
                if let Some(i) = n.as_i64() {
                    Ok(Val::S64(i))
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected signed integer for s64 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("F32") || t.contains("f32") => {
                if let Some(f) = n.as_f64() {
                    Ok(Val::Float32(f as f32))
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected float for f32 type".to_string(),
                    ))
                }
            }
            (Value::Number(n), t) if t.contains("F64") || t.contains("f64") => {
                if let Some(f) = n.as_f64() {
                    Ok(Val::Float64(f))
                } else {
                    Err(WasiMcpError::InvalidArguments(
                        "Expected float for f64 type".to_string(),
                    ))
                }
            }
            (Value::String(_), t) if t.contains("String") || t.contains("string") => {
                Self::json_to_wasm(json_value)
            }
            (Value::Array(_), t) if t.contains("List") || t.contains("Vec") || t.contains("[]") => {
                Self::json_to_wasm(json_value)
            }
            (Value::Object(_), t) if t.contains("Record") || t.contains("Tuple") => {
                Self::json_to_wasm(json_value)
            }
            (Value::Null, t) if t.contains("Option") => Ok(Val::Option(None)),
            _ => {
                // Fallback to basic conversion without type checking
                Self::json_to_wasm(json_value)
            }
        }
    }

    /// Convert WASM result values to JSON with proper formatting
    pub fn convert_wasm_results_to_json(wasm_results: &[Val]) -> Result<Value> {
        match wasm_results.len() {
            0 => Ok(Value::String(
                "Successfully executed (no return value)".to_string(),
            )),
            1 => Self::wasm_to_json(&wasm_results[0]),
            _ => {
                let json_results: Result<Vec<Value>> =
                    wasm_results.iter().map(Self::wasm_to_json).collect();
                Ok(Value::Array(json_results?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_bool_to_wasm() {
        let json_val = Value::Bool(true);
        let wasm_val = ValueTransformer::json_to_wasm(&json_val).unwrap();
        assert_eq!(wasm_val, Val::Bool(true));
    }

    #[test]
    fn test_wasm_bool_to_json() {
        let wasm_val = Val::Bool(false);
        let json_val = ValueTransformer::wasm_to_json(&wasm_val).unwrap();
        assert_eq!(json_val, Value::Bool(false));
    }

    #[test]
    fn test_json_number_to_wasm() {
        let json_val = Value::Number(serde_json::Number::from(42));
        let wasm_val = ValueTransformer::json_to_wasm(&json_val).unwrap();
        assert_eq!(wasm_val, Val::S64(42));
    }

    #[test]
    fn test_json_string_to_wasm() {
        let json_val = Value::String("hello".to_string());
        let wasm_val = ValueTransformer::json_to_wasm(&json_val).unwrap();
        assert_eq!(wasm_val, Val::String("hello".to_string()));
    }

    #[test]
    fn test_json_array_to_wasm() {
        let json_val = json!([1, 2, 3]);
        let wasm_val = ValueTransformer::json_to_wasm(&json_val).unwrap();
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
        let wasm_val = ValueTransformer::json_to_wasm(&json_val).unwrap();
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
        let json_val = ValueTransformer::wasm_to_json(&wasm_val).unwrap();
        assert_eq!(json_val, json!(["a", "b"]));
    }

    #[test]
    fn test_wasm_record_to_json() {
        let wasm_val = Val::Record(vec![
            ("name".to_string(), Val::String("test".to_string())),
            ("value".to_string(), Val::U32(42)),
        ]);
        let json_val = ValueTransformer::wasm_to_json(&wasm_val).unwrap();
        assert_eq!(json_val, json!({"name": "test", "value": 42}));
    }

    #[test]
    fn test_type_checked_conversion() {
        let json_val = Value::Number(serde_json::Number::from(100));
        let wasm_val = ValueTransformer::json_to_wasm_with_type(&json_val, "u8").unwrap();
        assert_eq!(wasm_val, Val::U8(100));

        // Test overflow
        let json_val = Value::Number(serde_json::Number::from(300));
        let result = ValueTransformer::json_to_wasm_with_type(&json_val, "u8");
        assert!(result.is_err());
    }

    #[test]
    fn test_result_conversion() {
        let wasm_val = Val::Result(Ok(Some(Box::new(Val::String("success".to_string())))));
        let json_val = ValueTransformer::wasm_to_json(&wasm_val).unwrap();

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
