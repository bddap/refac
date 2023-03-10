use serde::{Deserialize, Serialize};
use xdg::BaseDirectories;

use crate::pretrain_sample::Sample;

#[derive(Serialize, Deserialize, Debug)]
pub struct Secrets {
    pub openai_api_key: String,
}

impl Secrets {
    pub fn load() -> anyhow::Result<Self> {
        let base = BaseDirectories::with_prefix("refac")?;
        let path = base
            .find_config_file("secrets.toml")
            .ok_or(anyhow::anyhow!(
                "No secrets.toml file found. Try logging in with 'refac login'.",
            ))?;
        let secrets = std::fs::read_to_string(path)?;
        let ret: Secrets = toml::from_str(&secrets)?;
        Ok(ret)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let base = BaseDirectories::with_prefix("refac")?;
        let path = base.place_config_file("secrets.toml")?;
        std::fs::write(path, toml::to_string(self)?)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FinetuneInput {
    pub sample: Vec<Sample>,
}

impl FinetuneInput {
    pub fn builtin() -> Self {
        toml::from_str(include_str!("default_finetune.toml")).unwrap()
    }

    pub fn to_jsonl(&self) -> String {
        let mut ret = String::new();
        for sample in &self.sample {
            if !sample.is_correct() {
                continue;
            }

            #[derive(Serialize)]
            struct JsonlLine<'a> {
                prompt: &'a str,
                completion: &'a str,
            }

            let line = JsonlLine {
                prompt: &sample.prompt(),
                completion: sample.completion(),
            };
            ret.push_str(&serde_json::to_string(&line).unwrap());
            ret.push('\n');
        }
        ret
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    /// The fine-tuned model to use.
    /// Must be in our account.
    pub model: String,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let base = BaseDirectories::with_prefix("refac")?;
        let path = base.find_config_file("config.toml").ok_or(anyhow::anyhow!(
            "No config.toml file found. Try finetuning in with 'refac finetune'.",
        ))?;
        let config = std::fs::read_to_string(path)?;
        let ret: Config = toml::from_str(&config)?;
        Ok(ret)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let base = BaseDirectories::with_prefix("refac")?;
        let path = base.place_config_file("secrets.toml")?;
        std::fs::write(path, toml::to_string(self)?)?;
        Ok(())
    }
}
