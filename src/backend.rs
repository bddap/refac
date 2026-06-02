//! The model-backend interface: one trait both providers implement, plus the
//! single place where a `Provider` choice is turned into a ready-to-call,
//! key-bearing backend.

use anyhow::Result;

use crate::agent::{Model, ToolSpec};
use crate::anthropic::{Anthropic, AnthropicAgent};
use crate::api::Message;
use crate::config_files::{Provider, Secrets};
use crate::openai::{Openai, OpenaiAgent};

/// A resolved model backend — provider, key, and model already settled. Callers
/// hand it refac's provider-agnostic [`Message`]s and get back the completion.
///
/// Resolved to `Box<dyn Backend>` so call sites depend only on the interface,
/// never on which provider answered. The trait is where the upcoming tool /
/// function-call round-trip lands (both providers support it), keeping the
/// edit loop provider-agnostic.
pub trait Backend {
    /// Send the conversation and return the model's text output.
    fn complete(&self, messages: &[Message]) -> Result<String>;
}

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

/// Turn a resolved provider + model into a callable rewrite backend.
pub fn resolve(provider: Provider, model: &str, secrets: &Secrets) -> Result<Box<dyn Backend>> {
    let key = key_for(provider, secrets)?;
    Ok(match provider {
        Provider::Anthropic => Box::new(Anthropic::new(key, model.to_string())),
        Provider::Openai => Box::new(Openai::new(key, model.to_string())),
    })
}

/// Build an edit-mode [`Model`] for the provider, seeded with the conversation
/// and the tools to expose.
pub fn resolve_agent(
    provider: Provider,
    model: &str,
    secrets: &Secrets,
    seed: &[Message],
    tools: &[ToolSpec],
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

    #[test]
    fn resolve_errors_without_a_key() {
        let secrets = Secrets::default();
        assert!(resolve(Provider::Anthropic, "m", &secrets).is_err());
        assert!(resolve(Provider::Openai, "m", &secrets).is_err());
    }

    #[test]
    fn resolve_succeeds_with_the_matching_key() {
        let secrets = Secrets {
            anthropic_api_key: Some("a".into()),
            openai_api_key: Some("o".into()),
        };
        assert!(resolve(Provider::Anthropic, "m", &secrets).is_ok());
        assert!(resolve(Provider::Openai, "m", &secrets).is_ok());
    }
}
