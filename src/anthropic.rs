//! Anthropic (Claude) Messages API backend.
//!
//! Talks to the REST API directly with `reqwest` (blocking, same as the OpenAI
//! client). Differences from OpenAI that this module handles:
//!   - auth via the `x-api-key` header (+ `anthropic-version`), not bearer auth
//!   - the system prompt is a top-level `system` field, not a `system`-role message
//!   - consecutive same-role messages (refac sends `user(selected)` +
//!     `user(transform)`) are grouped into one turn
//!   - prompt caching: the caller-supplied static prefix (system prompt +
//!     few-shot examples) is marked `cache_control: ephemeral` so repeated calls
//!     only pay for the varying input

use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::Message;

/// `max_tokens` is required by the Messages API. It isn't a user-facing setting
/// (a config knob that only one provider honors is a representable invalid
/// state); hardcode a generous ceiling here.
const MAX_TOKENS: u32 = 16000;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: &'static str, // "ephemeral"
}

impl CacheControl {
    fn ephemeral() -> Self {
        CacheControl { kind: "ephemeral" }
    }
}

#[derive(Serialize)]
struct TextBlock {
    #[serde(rename = "type")]
    kind: &'static str, // "text"
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

impl TextBlock {
    fn new(text: impl Into<String>) -> Self {
        TextBlock {
            kind: "text",
            text: text.into(),
            cache_control: None,
        }
    }
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: Vec<TextBlock>,
}

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    system: Vec<TextBlock>,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ResponseBlock>,
}

#[derive(Deserialize)]
struct ResponseBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

/// Send a chat-style prompt to the Claude Messages API and return the text.
///
/// `messages` is refac's flat message list; the leading `cache_prefix_len` of
/// them are the static prefix (see `build_request`).
pub fn complete(
    api_key: &str,
    model: &str,
    messages: &[Message],
    cache_prefix_len: usize,
) -> anyhow::Result<String> {
    let req = build_request(model, messages, cache_prefix_len);

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
        .filter(|b| b.kind == "text")
        .map(|b| b.text)
        .collect();

    if text.is_empty() {
        return Err(anyhow::anyhow!("Anthropic returned no text content."));
    }

    Ok(text)
}

/// `cache_prefix_len` is the number of leading `messages` the caller considers
/// static — the prompt-caching breakpoint goes at the end of that prefix. The
/// caller owns this because only it knows what's fixed vs. per-call; the backend
/// doesn't infer it from message structure.
fn build_request(model: &str, messages: &[Message], cache_prefix_len: usize) -> MessagesRequest {
    let mut system_text = String::new();
    let mut convo: Vec<ChatMessage> = Vec::new();
    // How many `convo` turns came from the cacheable prefix.
    let mut prefix_turns: Option<usize> = None;

    for (i, m) in messages.iter().enumerate() {
        if i == cache_prefix_len {
            prefix_turns = Some(convo.len());
        }
        // Anthropic 400s on empty text blocks (some few-shot samples have an
        // empty `selected`); the OpenAI path tolerated them.
        if m.content.is_empty() {
            continue;
        }
        if m.role == "system" {
            if !system_text.is_empty() {
                system_text.push_str("\n\n");
            }
            system_text.push_str(&m.content);
            continue;
        }
        // Group consecutive same-role messages into one turn (refac sends the
        // selected text and the transform instruction as two user turns). Never
        // group across the prefix boundary, so the cached prefix turn can't
        // absorb varying input.
        match convo.last_mut() {
            Some(last) if last.role == m.role && i != cache_prefix_len => {
                last.content.push(TextBlock::new(&m.content))
            }
            _ => convo.push(ChatMessage {
                role: m.role.clone(),
                content: vec![TextBlock::new(&m.content)],
            }),
        }
    }
    let prefix_turns = prefix_turns.unwrap_or(convo.len());

    let mut system = Vec::new();
    if !system_text.is_empty() {
        let mut block = TextBlock::new(system_text);
        block.cache_control = Some(CacheControl::ephemeral());
        system.push(block);
    }
    // Cache through the last turn of the prefix; everything after it varies per
    // call and stays uncached.
    if let Some(block) = prefix_turns
        .checked_sub(1)
        .and_then(|idx| convo.get_mut(idx))
        .and_then(|turn| turn.content.last_mut())
    {
        block.cache_control = Some(CacheControl::ephemeral());
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

    #[test]
    fn build_request_shapes_anthropic_payload() {
        // Mirrors what refac sends: system + one few-shot (user,user,assistant)
        // then the two trailing user turns (selected, transform).
        let msgs = vec![
            Message::system("SYS"),
            Message::user("ex_selected"),
            Message::user("ex_transform"),
            Message::assistant("ex_result"),
            Message::user("real_selected"),
            Message::user("real_transform"),
        ];

        let req = build_request("claude-opus-4-8", &msgs, 4);
        let v = serde_json::to_value(&req).unwrap();

        assert_eq!(v["model"], "claude-opus-4-8");
        assert_eq!(v["max_tokens"], 16000);

        // System is lifted out of messages and cached.
        assert_eq!(v["system"][0]["text"], "SYS");
        assert_eq!(v["system"][0]["cache_control"]["type"], "ephemeral");

        // Consecutive same-role turns are merged → user, assistant, user (alternates).
        let m = v["messages"].as_array().unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[0]["role"], "user");
        assert_eq!(m[0]["content"].as_array().unwrap().len(), 2); // two few-shot user blocks
        assert_eq!(m[1]["role"], "assistant");
        assert_eq!(m[2]["role"], "user");
        assert_eq!(m[2]["content"].as_array().unwrap().len(), 2); // selected + transform

        // Cache breakpoint on the last few-shot assistant turn; the varying final
        // user input is NOT cached.
        assert_eq!(m[1]["content"][0]["cache_control"]["type"], "ephemeral");
        assert!(m[2]["content"][1].get("cache_control").is_none());
    }

    #[test]
    fn empty_text_blocks_are_dropped() {
        // A few-shot sample with an empty `selected` must not produce an empty
        // text block (Anthropic 400s on those).
        let msgs = vec![
            Message::user(""),
            Message::user("write hello world"),
            Message::assistant("print('hello world')"),
            Message::user("real input"),
            Message::user(""),
        ];
        let req = build_request("claude-opus-4-8", &msgs, 3);
        let v = serde_json::to_value(&req).unwrap();
        // No empty text anywhere.
        let s = serde_json::to_string(&v).unwrap();
        assert!(!s.contains(r#""text":"""#), "empty text block leaked: {s}");
        let m = v["messages"].as_array().unwrap();
        assert_eq!(m[0]["role"], "user");
        assert_eq!(m[0]["content"][0]["text"], "write hello world");
        assert_eq!(m[1]["role"], "assistant");
        assert_eq!(m[2]["content"][0]["text"], "real input");
    }

    #[test]
    fn no_system_yields_empty_system() {
        let msgs = vec![Message::user("hi")];
        let req = build_request("claude-opus-4-8", &msgs, 0);
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("system").is_none()); // skipped when empty
        assert_eq!(v["messages"][0]["role"], "user");
    }
}
