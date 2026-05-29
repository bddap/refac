//! Anthropic (Claude) Messages API backend.
//!
//! No official Rust SDK exists, so this talks to the REST API directly with
//! `reqwest` (blocking, same as the OpenAI client). Differences from OpenAI that
//! this module handles:
//!   - auth via the `x-api-key` header (+ `anthropic-version`), not bearer auth
//!   - the system prompt is a top-level `system` field, not a `system`-role message
//!   - messages must alternate user/assistant, so consecutive same-role messages
//!     (refac sends `user(selected)` + `user(transform)`) are merged into one turn
//!   - prompt caching: the static system prompt + few-shot examples are marked
//!     `cache_control: ephemeral` so repeated calls only pay for the varying input

use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::Message;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ToolChoice>,
}

#[derive(Serialize)]
struct Tool {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ToolChoice {
    Tool { name: &'static str },
}

/// One exact-substring replacement, as returned by the `apply_edits` tool.
#[derive(Debug, Deserialize)]
pub struct Edit {
    pub old: String,
    pub new: String,
}

#[derive(Deserialize)]
struct EditInput {
    edits: Vec<Edit>,
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
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<Value>,
}

/// Send a chat-style prompt to the Claude Messages API and return the text.
///
/// `messages` is refac's flat message list (system + few-shot user/assistant
/// pairs + the trailing user turns); this splits out the system prompt, merges
/// consecutive same-role turns to satisfy Anthropic's alternation requirement,
/// and caches the static prefix.
pub fn complete(
    api_key: &str,
    model: &str,
    max_tokens: u32,
    messages: &[Message],
) -> anyhow::Result<String> {
    let req = build_request(model, max_tokens, messages);
    let body = send(api_key, &req)?;

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

/// Ask Claude to express its changes as a list of exact-substring edits via the
/// `apply_edits` tool, instead of re-emitting the whole text. The caller applies
/// the returned edits to the original input.
pub fn request_edits(
    api_key: &str,
    model: &str,
    max_tokens: u32,
    messages: &[Message],
) -> anyhow::Result<Vec<Edit>> {
    let mut req = build_request(model, max_tokens, messages);
    req.tools = Some(vec![Tool {
        name: "apply_edits",
        description: "Apply edits to the selected text as a list of exact-substring \
            replacements. Each `old` MUST appear verbatim in the selected text. \
            Make the smallest edits that satisfy the request; do not restate \
            unchanged text. To insert, use a nearby unique substring as `old` and \
            set `new` to that substring plus your addition. Edits apply in order.",
        input_schema: edit_schema(),
    }]);
    req.tool_choice = Some(ToolChoice::Tool { name: "apply_edits" });

    let body = send(api_key, &req)?;

    let parsed: MessagesResponse = serde_json::from_value(body.clone())
        .map_err(|e| anyhow::anyhow!("Error while parsing response: {e} Body: {body}"))?;

    let input = parsed
        .content
        .into_iter()
        .find(|b| b.kind == "tool_use" && b.name.as_deref() == Some("apply_edits"))
        .and_then(|b| b.input)
        .ok_or_else(|| anyhow::anyhow!("Anthropic did not return an apply_edits tool call. Body: {body}"))?;

    let edits: EditInput = serde_json::from_value(input)
        .map_err(|e| anyhow::anyhow!("Error parsing apply_edits input: {e}"))?;

    Ok(edits.edits)
}

fn edit_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "edits": {
                "type": "array",
                "description": "Ordered list of substring replacements.",
                "items": {
                    "type": "object",
                    "properties": {
                        "old": { "type": "string", "description": "Exact substring to replace; must occur verbatim in the input." },
                        "new": { "type": "string", "description": "Replacement text." }
                    },
                    "required": ["old", "new"]
                }
            }
        },
        "required": ["edits"]
    })
}

/// POST a request to the Messages API and return the parsed JSON body, erroring
/// on non-2xx status.
fn send(api_key: &str, req: &MessagesRequest) -> anyhow::Result<Value> {
    if std::env::var("REFAC_DEBUG").is_ok() {
        eprintln!("{}", serde_json::to_string_pretty(req).unwrap_or_default());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60 * 4))
        .build()
        .context("building HTTP client")?;

    let response = client
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(req)
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

    Ok(body)
}

fn build_request(model: &str, max_tokens: u32, messages: &[Message]) -> MessagesRequest {
    let mut system_text = String::new();
    let mut convo: Vec<ChatMessage> = Vec::new();

    for m in messages {
        // Anthropic rejects empty text blocks (some few-shot samples have an empty
        // `selected`); the OpenAI path tolerated them. Drop empties here.
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
        // Merge consecutive same-role messages — Anthropic requires alternation,
        // and refac sends two user turns (selected, then transform) back to back.
        match convo.last_mut() {
            Some(last) if last.role == m.role => last.content.push(TextBlock::new(&m.content)),
            _ => convo.push(ChatMessage {
                role: m.role.clone(),
                content: vec![TextBlock::new(&m.content)],
            }),
        }
    }

    // Cache the static prefix. A breakpoint on the system block caches the system
    // prompt; a breakpoint on the last few-shot assistant turn caches everything
    // through the examples (render order is system → messages). The trailing user
    // input after it stays uncached, which is exactly what varies per call.
    let mut system = Vec::new();
    if !system_text.is_empty() {
        let mut block = TextBlock::new(system_text);
        block.cache_control = Some(CacheControl::ephemeral());
        system.push(block);
    }
    if let Some(idx) = convo.iter().rposition(|m| m.role == "assistant") {
        if let Some(block) = convo[idx].content.last_mut() {
            block.cache_control = Some(CacheControl::ephemeral());
        }
    }

    MessagesRequest {
        model: model.to_string(),
        max_tokens,
        system,
        messages: convo,
        tools: None,
        tool_choice: None,
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

        let req = build_request("claude-opus-4-8", 16000, &msgs);
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
        let req = build_request("claude-opus-4-8", 100, &msgs);
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
        let req = build_request("claude-opus-4-8", 100, &msgs);
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("system").is_none()); // skipped when empty
        assert_eq!(v["messages"][0]["role"], "user");
    }
}
