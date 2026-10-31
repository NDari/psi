//! Tool parameter schemas: one definition gives the JSON schema the model
//! sees and the rules the shape-fix pipeline applies. Schemas stay flat (H-05).

use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    String,
    Integer,
    Boolean,
    /// Extension tools only (core tools keep to the three above).
    Number,
    Array,
    Object,
}

impl ParamKind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "string" => Self::String,
            "integer" => Self::Integer,
            "boolean" => Self::Boolean,
            "number" => Self::Number,
            "array" => Self::Array,
            "object" => Self::Object,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: &'static str,
    pub kind: ParamKind,
    pub required: bool,
    pub description: &'static str,
    /// A file path: `~` is expanded and doubled working directories refused.
    pub is_path: bool,
}

impl Param {
    pub const fn new(name: &'static str, kind: ParamKind, description: &'static str) -> Self {
        Self {
            name,
            kind,
            required: true,
            description,
            is_path: false,
        }
    }

    pub const fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub const fn path(mut self) -> Self {
        self.is_path = true;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: &'static str,
    pub description: String,
    pub params: Vec<Param>,
    /// One correct call, shown when a call is refused.
    pub example: Value,
}

impl ToolSchema {
    /// The `tools` entry for a chat request.
    pub fn to_openai(&self) -> Value {
        let mut props = Map::new();
        for p in &self.params {
            let ty = match p.kind {
                ParamKind::String => "string",
                ParamKind::Integer => "integer",
                ParamKind::Boolean => "boolean",
                ParamKind::Number => "number",
                ParamKind::Array => "array",
                ParamKind::Object => "object",
            };
            props.insert(
                p.name.into(),
                json!({"type": ty, "description": p.description}),
            );
        }
        let required: Vec<&str> = self
            .params
            .iter()
            .filter(|p| p.required)
            .map(|p| p.name)
            .collect();
        json!({"type": "function", "function": {
            "name": self.name,
            "description": self.description,
            "parameters": {"type": "object", "properties": props, "required": required},
        }})
    }

    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }

    /// "Example: read {...}" for refusals.
    pub fn example_line(&self) -> String {
        format!("Example: {} {}", self.name, self.example)
    }
}
