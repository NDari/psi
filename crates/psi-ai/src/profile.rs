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
    /// Sampling fields sent with every request, by the server's names
    /// (llama-server: `temperature`, `top_p`, `top_k`, `min_p`,
    /// `presence_penalty`, `repeat_penalty`): the model card's values, kept
    /// with the model. Empty leaves the server's own settings in force.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub sampling: serde_json::Map<String, serde_json::Value>,
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
            sampling: serde_json::Map::new(),
            headers: Vec::new(),
        }
    }

    /// What is wrong with the profile's sampling, if anything: each field
    /// is a number, and none is one of the request's own fields.
    pub fn check_sampling(&self) -> Result<(), String> {
        const OWN: [&str; 8] = [
            "model",
            "messages",
            "tools",
            "stream",
            "stream_options",
            "max_tokens",
            "chat_template_kwargs",
            "return_progress",
        ];
        for (k, v) in &self.sampling {
            if OWN.contains(&k.as_str()) {
                return Err(format!(
                    "model {}: sampling can't set \"{k}\", a field of the request itself",
                    self.id
                ));
            }
            if !v.is_number() {
                return Err(format!(
                    "model {}: sampling.{k} must be a number, not {v}",
                    self.id
                ));
            }
        }
        Ok(())
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
    fn sampling_is_numbers_by_the_servers_names() {
        let p: ModelProfile = serde_json::from_value(json!({"id": "q",
            "sampling": {"temperature": 0.7, "top_p": 0.8, "top_k": 20, "presence_penalty": 1.5}}))
        .unwrap();
        assert_eq!(p.sampling["top_k"], 20);
        assert_eq!(p.check_sampling(), Ok(()));
        assert!(ModelProfile::local().sampling.is_empty());
        let bad: ModelProfile =
            serde_json::from_value(json!({"id": "q", "sampling": {"temperature": "hot"}})).unwrap();
        assert_eq!(
            bad.check_sampling().unwrap_err(),
            "model q: sampling.temperature must be a number, not \"hot\""
        );
        let bad: ModelProfile =
            serde_json::from_value(json!({"id": "q", "sampling": {"max_tokens": 9}})).unwrap();
        assert!(
            bad.check_sampling()
                .unwrap_err()
                .contains("a field of the request itself")
        );
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
