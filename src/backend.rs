//! Turning a `Provider` choice into a ready-to-run, key-bearing edit-mode model.

use anyhow::Result;

use crate::agent::{Model, Seed, Tool};
use crate::anthropic::AnthropicAgent;
use crate::config_files::{Provider, Secrets};
use crate::openai::OpenaiAgent;

/// The one spot that knows how each provider sources its API key. Fails if the
/// chosen provider's key is missing, so the rest of refac stays provider-agnostic.
fn key_for(provider: Provider, secrets: &Secrets) -> Result<String> {
    match provider {
        Provider::Anthropic => secrets.anthropic_api_key.clone().ok_or_else(|| {
            anyhow::anyhow!("No Anthropic API key found. Set ANTHROPIC_API_KEY or run 'refac login'.")
        }),
        Provider::Openai => secrets.openai_api_key.clone().ok_or_else(|| {
            anyhow::anyhow!("No OpenAI API key found. Set OPENAI_API_KEY or run 'refac login'.")
        }),
    }
}

/// Build an edit-mode [`Model`] for the provider, seeded with the conversation
/// and the tools to expose.
pub fn resolve_agent(
    provider: Provider,
    model: &str,
    secrets: &Secrets,
    seed: &Seed,
    tools: &[Tool],
) -> Result<Box<dyn Model>> {
    let key = key_for(provider, secrets)?;
    Ok(match provider {
        Provider::Anthropic => Box::new(AnthropicAgent::new(key, model.to_string(), seed, tools)),
        Provider::Openai => Box::new(OpenaiAgent::new(key, model.to_string(), seed, tools)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<Tool> {
        crate::agent::tools()
    }

    fn seed() -> Seed<'static> {
        Seed {
            system: "s",
            selected: "x",
            transform: "y",
        }
    }

    #[test]
    fn resolve_agent_errors_without_a_key() {
        let secrets = Secrets::default();
        assert!(resolve_agent(Provider::Anthropic, "m", &secrets, &seed(), &tools()).is_err());
        assert!(resolve_agent(Provider::Openai, "m", &secrets, &seed(), &tools()).is_err());
    }

    #[test]
    fn resolve_agent_succeeds_with_the_matching_key() {
        let secrets = Secrets {
            anthropic_api_key: Some("a".into()),
            openai_api_key: Some("o".into()),
        };
        assert!(resolve_agent(Provider::Anthropic, "m", &secrets, &seed(), &tools()).is_ok());
        assert!(resolve_agent(Provider::Openai, "m", &secrets, &seed(), &tools()).is_ok());
    }
}
