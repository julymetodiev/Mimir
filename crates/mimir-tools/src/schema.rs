//! JSON Schema generation for tool parameters.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// JSON Schema type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JsonSchemaType {
    String,
    Number,
    Integer,
    Boolean,
    Array,
    Object,
    Null,
}

/// JSON Schema definition.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JsonSchema {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<JsonSchemaType>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, JsonSchema>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<JsonSchema>>,

    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<Value>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,

    #[serde(rename = "minLength", skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,

    #[serde(rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<i64>,
}

impl JsonSchema {
    /// Create a string schema.
    pub fn string() -> Self {
        Self {
            schema_type: Some(JsonSchemaType::String),
            ..Default::default()
        }
    }

    /// Create an integer schema.
    pub fn integer() -> Self {
        Self {
            schema_type: Some(JsonSchemaType::Integer),
            ..Default::default()
        }
    }

    /// Create a number schema.
    pub fn number() -> Self {
        Self {
            schema_type: Some(JsonSchemaType::Number),
            ..Default::default()
        }
    }

    /// Create a boolean schema.
    pub fn boolean() -> Self {
        Self {
            schema_type: Some(JsonSchemaType::Boolean),
            ..Default::default()
        }
    }

    /// Create an array schema.
    pub fn array(items: JsonSchema) -> Self {
        Self {
            schema_type: Some(JsonSchemaType::Array),
            items: Some(Box::new(items)),
            ..Default::default()
        }
    }

    /// Create an object schema.
    pub fn object() -> Self {
        Self {
            schema_type: Some(JsonSchemaType::Object),
            properties: Some(HashMap::new()),
            ..Default::default()
        }
    }

    /// Add a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a property to an object schema.
    pub fn with_property(mut self, name: impl Into<String>, schema: JsonSchema) -> Self {
        if self.properties.is_none() {
            self.properties = Some(HashMap::new());
        }
        self.properties.as_mut().unwrap().insert(name.into(), schema);
        self
    }

    /// Mark properties as required.
    pub fn with_required(mut self, required: Vec<&str>) -> Self {
        self.required = Some(required.into_iter().map(String::from).collect());
        self
    }

    /// Set enum values.
    pub fn with_enum(mut self, values: Vec<&str>) -> Self {
        self.enum_values = Some(values.into_iter().map(|s| Value::String(s.to_string())).collect());
        self
    }

    /// Set default value.
    pub fn with_default(mut self, default: impl Serialize) -> Self {
        self.default = serde_json::to_value(default).ok();
        self
    }
}

/// Tool parameter definition (convenience wrapper).
#[derive(Debug, Clone)]
pub struct ToolParameter {
    pub name: String,
    pub schema: JsonSchema,
    pub required: bool,
}

impl ToolParameter {
    /// Create a required string parameter.
    pub fn required_string(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: JsonSchema::string().with_description(description),
            required: true,
        }
    }

    /// Create an optional string parameter.
    pub fn optional_string(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: JsonSchema::string().with_description(description),
            required: false,
        }
    }

    /// Create a required integer parameter.
    pub fn required_integer(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: JsonSchema::integer().with_description(description),
            required: true,
        }
    }

    /// Create an optional integer parameter.
    pub fn optional_integer(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: JsonSchema::integer().with_description(description),
            required: false,
        }
    }

    /// Create a required boolean parameter.
    pub fn required_bool(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: JsonSchema::boolean().with_description(description),
            required: true,
        }
    }

    /// Create an optional boolean parameter with default.
    pub fn optional_bool(name: impl Into<String>, description: impl Into<String>, default: bool) -> Self {
        Self {
            name: name.into(),
            schema: JsonSchema::boolean().with_description(description).with_default(default),
            required: false,
        }
    }
}

/// Complete tool schema including name, description, and parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: JsonSchema,
}

impl ToolSchema {
    /// Create a new tool schema.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: JsonSchema::object(),
        }
    }

    /// Add a parameter.
    pub fn with_param(mut self, param: ToolParameter) -> Self {
        self.parameters = self.parameters.with_property(&param.name, param.schema);
        if param.required {
            let mut required = self.parameters.required.unwrap_or_default();
            required.push(param.name);
            self.parameters.required = Some(required);
        }
        self
    }

    /// Convert to Anthropic tool format.
    pub fn to_anthropic_format(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.parameters
        })
    }

    /// Convert to OpenAI function format.
    pub fn to_openai_format(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_schema_string() {
        let schema = JsonSchema::string().with_description("A name");
        assert_eq!(schema.schema_type, Some(JsonSchemaType::String));
        assert_eq!(schema.description, Some("A name".to_string()));
    }

    #[test]
    fn test_tool_schema() {
        let schema = ToolSchema::new("read_file", "Read a file from disk")
            .with_param(ToolParameter::required_string("path", "File path to read"))
            .with_param(ToolParameter::optional_integer("limit", "Max lines to read"));

        assert_eq!(schema.name, "read_file");
        assert!(schema.parameters.properties.as_ref().unwrap().contains_key("path"));
        assert_eq!(schema.parameters.required, Some(vec!["path".to_string()]));
    }

    #[test]
    fn test_anthropic_format() {
        let schema = ToolSchema::new("test", "A test tool")
            .with_param(ToolParameter::required_string("input", "Input value"));

        let json = schema.to_anthropic_format();
        assert_eq!(json["name"], "test");
        assert!(json["input_schema"]["properties"]["input"].is_object());
    }
}
