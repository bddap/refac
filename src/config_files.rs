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
        let contents = toml::to_string(self)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?
                .write_all(contents.as_bytes())?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(not(unix))]
        fs::write(&path, contents)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    Openai,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Config {
    #[serde(default)]
    pub provider: Option<Provider>,
    #[serde(default)]
    pub model: Option<String>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let mut ret: Config = match base()?.find_config_file("config.toml") {
            Some(path) => toml::from_str(&fs::read_to_string(path)?)?,
            None => Config::default(),
        };
        if let Ok(from_env) = std::env::var("REFAC_PROVIDER") {
            let provider = clap::ValueEnum::from_str(&from_env, true)
                .map_err(|e| anyhow::anyhow!("invalid REFAC_PROVIDER: {e}"))?;
            ret.provider = Some(provider);
        }
        if let Ok(from_env) = std::env::var("REFAC_MODEL") {
            ret.model = Some(from_env);
        }
        Ok(ret)
    }

    pub fn provider(&self, secrets: &Secrets) -> Provider {
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
        let cfg = Config::default();
        assert_eq!(cfg.provider(&secrets(false, true)), Provider::Openai);
        assert_eq!(cfg.provider(&secrets(true, false)), Provider::Anthropic);
        assert_eq!(cfg.provider(&secrets(true, true)), Provider::Anthropic);
        assert_eq!(cfg.provider(&secrets(false, false)), Provider::Anthropic);
    }

    #[test]
    fn explicit_provider_overrides_inference() {
        let cfg = Config {
            provider: Some(Provider::Openai),
            ..Config::default()
        };
        assert_eq!(cfg.provider(&secrets(true, false)), Provider::Openai);
    }
}
