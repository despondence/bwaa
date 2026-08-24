use crate::googleapis::google::ai::generativelanguage::v1beta::{Schema, Type};
use prost_types::value::Kind;
use prost_types::{ListValue, Struct, Value as ProstValue};
use serde_json::{Map, Value as JsonValue};
use std::collections::HashMap;

pub fn json_to_prost_struct(json: &JsonValue) -> Struct {
    match json {
        JsonValue::Object(map) => Struct {
            fields: map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_prost_value(v)))
                .collect(),
        },
        _ => Struct::default(),
    }
}

pub fn json_to_prost_value(json: &JsonValue) -> ProstValue {
    let kind = match json {
        JsonValue::Null => Kind::NullValue(0),
        JsonValue::Bool(b) => Kind::BoolValue(*b),
        JsonValue::Number(n) => {
            if let Some(f) = n.as_f64() {
                Kind::NumberValue(f)
            } else {
                Kind::NullValue(0)
            }
        }
        JsonValue::String(s) => Kind::StringValue(s.clone()),
        JsonValue::Array(arr) => Kind::ListValue(ListValue {
            values: arr.iter().map(json_to_prost_value).collect(),
        }),
        JsonValue::Object(_) => Kind::StructValue(json_to_prost_struct(json)),
    };
    ProstValue { kind: Some(kind) }
}

pub fn prost_struct_to_json(proto: &Struct) -> JsonValue {
    let mut map = Map::new();
    for (k, v) in &proto.fields {
        map.insert(k.clone(), prost_value_to_json(v));
    }
    JsonValue::Object(map)
}

pub fn prost_value_to_json(proto: &ProstValue) -> JsonValue {
    match &proto.kind {
        Some(Kind::NullValue(_)) | None => JsonValue::Null,
        Some(Kind::BoolValue(b)) => JsonValue::Bool(*b),
        Some(Kind::NumberValue(n)) => {
            serde_json::Number::from_f64(*n).map_or(JsonValue::Null, JsonValue::Number)
        }
        Some(Kind::StringValue(s)) => JsonValue::String(s.clone()),
        Some(Kind::ListValue(l)) => {
            JsonValue::Array(l.values.iter().map(prost_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => prost_struct_to_json(s),
    }
}

pub fn json_to_gemini_schema(json: &JsonValue) -> Schema {
    let mut schema = Schema::default();

    if let Some(obj) = json.as_object() {
        if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
            schema.r#type = match t {
                "string" => Type::String as i32,
                "number" => Type::Number as i32,
                "integer" => Type::Integer as i32,
                "boolean" => Type::Boolean as i32,
                "array" => Type::Array as i32,
                "object" => Type::Object as i32,
                _ => Type::Unspecified as i32,
            };
        }

        if let Some(desc) = obj.get("description").and_then(|v| v.as_str()) {
            schema.description = desc.to_string();
        }

        if let Some(nullable) = obj.get("nullable").and_then(|v| v.as_bool()) {
            schema.nullable = nullable;
        }

        if let Some(req_arr) = obj.get("required").and_then(|v| v.as_array()) {
            schema.required = req_arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }

        if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
            let mut prop_map = HashMap::new();
            for (k, v) in props {
                prop_map.insert(k.clone(), json_to_gemini_schema(v));
            }
            schema.properties = prop_map;
        }

        if let Some(items) = obj.get("items") {
            schema.items = Some(Box::new(json_to_gemini_schema(items)));
        }
    }

    schema
}
