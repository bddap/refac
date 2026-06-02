//! The model-backend interface: one trait both providers implement, plus the
//! single place where a `Provider` choice is turned into a ready-to-call,
//! key-bearing backend.

use anyhow::Result;

use crate::anthropic::Anthropic;
use crate::api::Message;
use crate::config_files::{Provider, Secrets};
use crate::openai::Openai;

/// A resolved model backend — provider, key, and model already settled. Callers
/// hand it refac's provider-agnostic [`Message`]s and get back the completion.
///
/// Returned as `Box<dyn Backend>` rather than a closed `enum` on purpose:
/// upcoming tool/function-call edits are an Anthropic-only capability, which a
/// trait expresses as a separate `Edits` trait that only `Anthropic` implements
/// — no enum arm that has to fake "unsupported" at runtime. Keep it `dyn`.
pub trait Backend {
    /// Send the conversation and return the model's text output.
    fn complete(&self, messages: &[Message]) -> Result<String>;
}

/// Turn a resolved provider + model into a callable backend, failing if that
/// provider's API key is missing. This is the one spot that knows how each
/// provider sources its key, so the rest of refac stays provider-agnostic.
pub fn resolve(provider: Provider, model: &str, secrets: &Secrets) -> Result<Box<dyn Backend>> {
    match provider {
        Provider::Anthropic => {
            let key = secrets.anthropic_api_key.clone().ok_or_else(|| {
                anyhow::anyhow!("No Anthropic API key found. Set ANTHROPIC_API_KEY or run 'refac login'.")
            })?;
            Ok(Box::new(Anthropic::new(key, model.to_string())))
        }
        Provider::Openai => {
            let key = secrets.openai_api_key.clone().ok_or_else(|| {
                anyhow::anyhow!("No OpenAI API key found. Set OPENAI_API_KEY or run 'refac login'.")
            })?;
            Ok(Box::new(Openai::new(key, model.to_string())))
        }
    }
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
