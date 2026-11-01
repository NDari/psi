//! Model profiles from `~/.psi/models.jsonc` (psi-plan.md §4.3).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub id: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// "auto" (from `/props`) or a token count.
    #[serde(default)]
    pub context_window: ContextWindow,
    #[serde(default = "default_max_output")]
    pub max_output: u64,
    #[serde(default)]
    pub reasoning: Reasoning,
    /// "endpoint" (the server's tokenizer) or "estimate" (D-20).
    #[serde(default = "default_tokenizer")]
    pub tokenizer: String,
    #[serde(default)]
    pub tools: ToolsConfig,
    /// Request headers, such as `Authorization` for a registered provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextWindow {
    #[default]
    Auto,
    Tokens(u64),
}

impl Serialize for ContextWindow {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ContextWindow::Auto => s.serialize_str("auto"),
            ContextWindow::Tokens(n) => s.serialize_u64(*n),
        }
    }
}

impl<'de> Deserialize<'de> for ContextWindow {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::String(s) if s == "auto" => Ok(ContextWindow::Auto),
            serde_json::Value::Number(n) => n
                .as_u64()
                .map(ContextWindow::Tokens)
                .ok_or_else(|| D::Error::custom("context_window must be a positive integer")),
            other => Err(D::Error::custom(format!(
                "context_window is \"auto\" or a number, not {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Reasoning {
    /// Sent as `chat_template_kwargs.enable_thinking` when set (H-09).
    #[serde(default)]
    pub enable_thinking: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolsConfig {
    /// The optional grep, find and ls tools (D-17, H-10).
    #[serde(default)]
    pub grep_find_ls: bool,
}

fn default_provider() -> String {
    "openai-compat".into()
}
fn default_base_url() -> String {
    "http://127.0.0.1:9931/v1".into()
}
fn default_max_output() -> u64 {
    8192
}
fn default_tokenizer() -> String {
    "endpoint".into()
}

impl ModelProfile {
    /// The profile used when models.jsonc has none: llama-server on 9931.
    pub fn local() -> Self {
        Self {
            id: "local".into(),
            provider: default_provider(),
            base_url: default_base_url(),
            context_window: ContextWindow::Auto,
            max_output: default_max_output(),
            reasoning: Reasoning::default(),
            tokenizer: default_tokenizer(),
            tools: ToolsConfig::default(),
            headers: Vec::new(),
        }
    }
}

/// The models.jsonc file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelsFile {
    #[serde(default)]
    pub models: Vec<ModelProfile>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_example_parses() {
        let v = json!({"models": [{
            "id": "qwen3.5-4b", "provider": "openai-compat",
            "base_url": "http://127.0.0.1:9931/v1", "context_window": "auto",
            "max_output": 4096, "tokenizer": "endpoint",
            "tools": {"grep_find_ls": false}
        }]});
        let f: ModelsFile = serde_json::from_value(v).unwrap();
        assert_eq!(f.models[0].context_window, ContextWindow::Auto);
        assert_eq!(f.models[0].max_output, 4096);
    }

    #[test]
    fn numeric_window_and_defaults() {
        let p: ModelProfile =
            serde_json::from_value(json!({"id": "x", "context_window": 8192})).unwrap();
        assert_eq!(p.context_window, ContextWindow::Tokens(8192));
        assert_eq!(p.base_url, "http://127.0.0.1:9931/v1");
        assert!(
            serde_json::from_value::<ModelProfile>(json!({"id": "x", "context_window": "big"}))
                .is_err()
        );
    }
}
