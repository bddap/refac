use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use xdg::BaseDirectories;

fn base() -> Result<BaseDirectories> {
    BaseDirectories::with_prefix("refac").map_err(Into::into)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Secrets {
    pub openai_api_key: String,
}

impl Secrets {
    pub fn load() -> anyhow::Result<Self> {
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            return Ok(Secrets {
                openai_api_key: api_key,
            });
        }
        let path = base()?
            .find_config_file("secrets.toml")
            .ok_or(anyhow::anyhow!(
                "No secrets.toml file found. Try logging in with 'refac login'.",
            ))?;
        let secrets = fs::read_to_string(path)?;
        let ret: Secrets = toml::from_str(&secrets)?;
        Ok(ret)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = base()?.place_config_file("secrets.toml")?;
        fs::write(path, toml::to_string(self)?)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_model() -> String {
    "o1".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            model: default_model(),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let mut ret = match base()?.find_config_file("config.toml") {
            Some(path) => {
                let config = fs::read_to_string(path)?;
                let ret: Config = toml::from_str(&config)?;
                ret
            }
            None => Config::default(),
        };
        if let Ok(from_env) = std::env::var("REFAC_MODEL") {
            ret.model = from_env;
        }
        Ok(ret)
    }
}
