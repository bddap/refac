use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use xdg::BaseDirectories;

fn base() -> Result<BaseDirectories> {
    BaseDirectories::with_prefix("refac").map_err(Into::into)
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Secrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
}

impl Secrets {
    /// Load secrets from `secrets.toml`, with env vars (`OPENAI_API_KEY`,
    /// `ANTHROPIC_API_KEY`) taking precedence. A missing file is not an error —
    /// env vars alone are enough.
    pub fn load() -> anyhow::Result<Self> {
        let mut secrets: Secrets = match base()?.find_config_file("secrets.toml") {
            Some(path) => toml::from_str(&fs::read_to_string(path)?)?,
            None => Secrets::default(),
        };
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            secrets.openai_api_key = Some(key);
        }
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            secrets.anthropic_api_key = Some(key);
        }
        Ok(secrets)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = base()?.place_config_file("secrets.toml")?;
        fs::write(path, toml::to_string(self)?)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Openai,
}

/// Default Claude model. Bump here when model ids churn.
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-4-8";

fn default_provider() -> Provider {
    Provider::Anthropic
}

/// How edits are produced. `Tool` (default, Anthropic only) returns structured
/// substring replacements via a function call; `Rewrite` re-emits the full text.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EditMode {
    Rewrite,
    Tool,
}

fn default_edit_mode() -> EditMode {
    EditMode::Tool
}

fn default_max_tokens() -> u32 {
    16000
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_provider")]
    pub provider: Provider,
    /// Model id. If unset, a sensible default is chosen per provider (see `model()`).
    #[serde(default)]
    pub model: Option<String>,
    /// Edit strategy. Tool-call edits (default) only apply on the Anthropic
    /// provider; the OpenAI path always rewrites.
    #[serde(default = "default_edit_mode")]
    pub edit_mode: EditMode,
    /// Max tokens to generate. Required by Anthropic; ignored by the OpenAI path.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            provider: default_provider(),
            model: None,
            edit_mode: default_edit_mode(),
            max_tokens: default_max_tokens(),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let mut ret: Config = match base()?.find_config_file("config.toml") {
            Some(path) => toml::from_str(&fs::read_to_string(path)?)?,
            None => Config::default(),
        };
        if let Ok(from_env) = std::env::var("REFAC_PROVIDER") {
            ret.provider = match from_env.to_lowercase().as_str() {
                "anthropic" => Provider::Anthropic,
                "openai" => Provider::Openai,
                other => anyhow::bail!("unknown REFAC_PROVIDER {other:?} (expected anthropic|openai)"),
            };
        }
        if let Ok(from_env) = std::env::var("REFAC_MODEL") {
            ret.model = Some(from_env);
        }
        if let Ok(from_env) = std::env::var("REFAC_EDIT_MODE") {
            ret.edit_mode = match from_env.to_lowercase().as_str() {
                "rewrite" => EditMode::Rewrite,
                "tool" => EditMode::Tool,
                other => anyhow::bail!("unknown REFAC_EDIT_MODE {other:?} (expected tool|rewrite)"),
            };
        }
        Ok(ret)
    }

    /// Resolve the model id, defaulting per provider when unset.
    pub fn model(&self) -> String {
        match &self.model {
            Some(m) => m.clone(),
            None => match self.provider {
                Provider::Anthropic => DEFAULT_ANTHROPIC_MODEL.to_string(),
                Provider::Openai => "o1".to_string(),
            },
        }
    }
}
