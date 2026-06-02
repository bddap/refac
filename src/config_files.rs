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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Openai,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    /// Explicit provider choice. When unset, it is inferred from which API keys
    /// are configured (see `resolve_provider`).
    #[serde(default)]
    pub provider: Option<Provider>,
    /// Model id. If unset, a sensible default is chosen per provider (see `model()`).
    #[serde(default)]
    pub model: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            provider: None,
            model: None,
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
            ret.provider = Some(match from_env.to_lowercase().as_str() {
                "anthropic" => Provider::Anthropic,
                "openai" => Provider::Openai,
                other => anyhow::bail!(
                    "invalid REFAC_PROVIDER {other:?}; expected \"anthropic\" or \"openai\""
                ),
            });
        }
        if let Ok(from_env) = std::env::var("REFAC_MODEL") {
            ret.model = Some(from_env);
        }
        Ok(ret)
    }

    /// Resolve the effective provider. An explicit choice (config file or
    /// `REFAC_PROVIDER`) always wins; otherwise infer from which API keys are
    /// configured, leaning Anthropic when both or neither are present.
    pub fn resolve_provider(&self, secrets: &Secrets) -> Provider {
        if let Some(p) = self.provider {
            return p;
        }
        match (
            secrets.anthropic_api_key.is_some(),
            secrets.openai_api_key.is_some(),
        ) {
            (false, true) => Provider::Openai,
            _ => Provider::Anthropic,
        }
    }

    pub fn model(&self, provider: Provider) -> String {
        match &self.model {
            Some(m) => m.clone(),
            None => match provider {
                Provider::Anthropic => "claude-opus-4-8".to_string(),
                Provider::Openai => "gpt-5.5".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets(anthropic: bool, openai: bool) -> Secrets {
        Secrets {
            anthropic_api_key: anthropic.then(|| "a".to_string()),
            openai_api_key: openai.then(|| "o".to_string()),
        }
    }

    #[test]
    fn provider_inferred_from_available_keys() {
        let cfg = Config::default(); // provider unset
        // Only OpenAI configured -> OpenAI.
        assert_eq!(cfg.resolve_provider(&secrets(false, true)), Provider::Openai);
        // Anthropic only, both, or neither -> lean Anthropic.
        assert_eq!(cfg.resolve_provider(&secrets(true, false)), Provider::Anthropic);
        assert_eq!(cfg.resolve_provider(&secrets(true, true)), Provider::Anthropic);
        assert_eq!(cfg.resolve_provider(&secrets(false, false)), Provider::Anthropic);
    }

    #[test]
    fn explicit_provider_overrides_inference() {
        let cfg = Config {
            provider: Some(Provider::Openai),
            ..Config::default()
        };
        // Explicit choice wins even when only an Anthropic key is present.
        assert_eq!(cfg.resolve_provider(&secrets(true, false)), Provider::Openai);
    }
}
