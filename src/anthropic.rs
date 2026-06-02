//! Anthropic (Claude) Messages API backend.

use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{field_or_placeholder, Message, Role};

const MAX_TOKENS: u32 = 80000;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum CacheControl {
    Ephemeral,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ContentBlock {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

impl ContentBlock {
    fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text {
            text: text.into(),
            cache_control: None,
        }
    }
}

#[derive(Serialize)]
struct ChatMessage {
    role: Role,
    content: Vec<ContentBlock>,
}

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    system: Vec<ContentBlock>,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ResponseBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ResponseBlock {
    Text { text: String },
    #[serde(other)]
    Other,
}

/// Send a chat-style prompt to the Claude Messages API and return the text.
pub fn complete(api_key: &str, model: &str, messages: &[Message]) -> anyhow::Result<String> {
    let req = build_request(model, messages);

    tracing::debug!(
        "anthropic request: {}",
        serde_json::to_string_pretty(&req).unwrap_or_default()
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60 * 4))
        .build()
        .context("building HTTP client")?;

    let response = client
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .context("Failed to send request to Anthropic API")?;

    let status = response.status();
    let body = response
        .json::<Value>()
        .with_context(|| anyhow::anyhow!("Status: {status}. Failed to parse response body."))?;

    if !status.is_success() {
        let pretty = serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string());
        return Err(anyhow::anyhow!("Status: {status}. Body: {pretty}"));
    }

    let parsed: MessagesResponse = serde_json::from_value(body.clone())
        .map_err(|e| anyhow::anyhow!("Error while parsing response: {e} Body: {body}"))?;

    let text: String = parsed
        .content
        .into_iter()
        .filter_map(|b| match b {
            ResponseBlock::Text { text } => Some(text),
            ResponseBlock::Other => None,
        })
        .collect();

    if text.is_empty() {
        return Err(anyhow::anyhow!("Anthropic returned no text content."));
    }

    Ok(text)
}

fn build_request(model: &str, messages: &[Message]) -> MessagesRequest {
    let mut system = Vec::new();
    let mut convo: Vec<ChatMessage> = Vec::new();

    for m in messages {
        let mut blocks: Vec<ContentBlock> = m
            .fields
            .iter()
            .map(|f| ContentBlock::text(field_or_placeholder(f)))
            .collect();
        // A cached turn caches everything up to and including its last block.
        if m.cache {
            if let Some(ContentBlock::Text { cache_control, .. }) = blocks.last_mut() {
                *cache_control = Some(CacheControl::Ephemeral);
            }
        }
        match m.role {
            Role::System => system.extend(blocks),
            Role::User | Role::Assistant => convo.push(ChatMessage {
                role: m.role,
                content: blocks,
            }),
        }
    }

    MessagesRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system,
        messages: convo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(fields: &[&str]) -> Message {
        Message::user(fields.iter().map(|f| f.to_string()).collect())
    }

    #[test]
    fn build_request_shapes_anthropic_payload() {
        let mut assistant = Message::assistant("ex_result");
        assistant.cache = true;
        let msgs = vec![
            Message::system("SYS"),
            user(&["ex_selected", "ex_transform"]),
            assistant,
            user(&["real_selected", "real_transform"]),
        ];

        let req = build_request("claude-opus-4-8", &msgs);
        let v = serde_json::to_value(&req).unwrap();

        assert_eq!(v["model"], "claude-opus-4-8");
        assert_eq!(v["max_tokens"], 80000);
        assert_eq!(v["system"][0]["text"], "SYS");

        let m = v["messages"].as_array().unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[0]["role"], "user");
        assert_eq!(m[0]["content"].as_array().unwrap().len(), 2);
        assert_eq!(m[1]["role"], "assistant");
        assert_eq!(m[2]["role"], "user");
        assert_eq!(m[2]["content"].as_array().unwrap().len(), 2);

        // The cached turn carries the breakpoint; the trailing input does not.
        assert_eq!(m[1]["content"][0]["cache_control"]["type"], "ephemeral");
        assert!(m[2]["content"][1].get("cache_control").is_none());
    }

    #[test]
    fn empty_fields_become_placeholder() {
        let req = build_request("claude-opus-4-8", &[user(&["", "transform"])]);
        let v = serde_json::to_value(&req).unwrap();
        let s = serde_json::to_string(&v).unwrap();
        assert!(!s.contains(r#""text":"""#), "empty text block leaked: {s}");
        assert_eq!(v["messages"][0]["content"][0]["text"], "(empty)");
        assert_eq!(v["messages"][0]["content"][1]["text"], "transform");
    }

    #[test]
    fn no_system_yields_empty_system() {
        let req = build_request("claude-opus-4-8", &[user(&["hi"])]);
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("system").is_none());
        assert_eq!(v["messages"][0]["role"], "user");
    }
}
