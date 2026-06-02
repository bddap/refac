//! Anthropic (Claude) Messages API backend.

use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::{Model, RawCall, ToolResult, ToolSpec};
use crate::api::{Message, Role};
use crate::backend::Backend;

const MAX_TOKENS: u32 = 80000;

/// The Anthropic backend: an API key and the model to call.
pub struct Anthropic {
    key: String,
    model: String,
}

impl Anthropic {
    pub fn new(key: String, model: String) -> Self {
        Anthropic { key, model }
    }
}

impl Backend for Anthropic {
    fn complete(&self, messages: &[Message]) -> anyhow::Result<String> {
        send(&self.key, &self.model, messages)
    }
}

/// Anthropic 400s on an empty text block, so render empty fields as a visible
/// placeholder.
fn field_or_placeholder(field: &str) -> &str {
    if field.is_empty() {
        "(empty)"
    } else {
        field
    }
}

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
fn send(api_key: &str, model: &str, messages: &[Message]) -> anyhow::Result<String> {
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

/// An edit-mode session against the Messages API. Implements [`Model`]: each
/// `turn` posts the running conversation plus the tool definitions and returns
/// the model's tool calls; `respond` threads the results back as a `tool_result`
/// user turn. The assistant's content is echoed back verbatim (as JSON) on the
/// next turn, which is what the API requires for a `tool_use`/`tool_result`
/// exchange.
pub struct AnthropicAgent {
    key: String,
    model: String,
    client: reqwest::blocking::Client,
    system: Vec<Value>,
    messages: Vec<Value>,
    tools: Vec<Value>,
}

impl AnthropicAgent {
    /// Seed from refac's provider-agnostic messages (system + the user turn) and
    /// the tools to expose.
    pub fn new(key: String, model: String, seed: &[Message], tools: &[ToolSpec]) -> Self {
        let mut system = Vec::new();
        let mut messages = Vec::new();
        for m in seed {
            let blocks: Vec<Value> = m
                .fields
                .iter()
                .map(|f| json!({ "type": "text", "text": field_or_placeholder(f) }))
                .collect();
            match m.role {
                Role::System => system.extend(blocks),
                Role::User | Role::Assistant => {
                    messages.push(json!({ "role": m.role.as_str(), "content": blocks }))
                }
            }
        }
        let tools = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        AnthropicAgent {
            key,
            model,
            client: crate::agent::http_client(),
            system,
            messages,
            tools,
        }
    }

    fn request(&self) -> Value {
        let mut req = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "messages": self.messages,
            "tools": self.tools,
            "tool_choice": { "type": "auto" },
        });
        if !self.system.is_empty() {
            req["system"] = json!(self.system);
        }
        req
    }
}

impl Model for AnthropicAgent {
    fn turn(&mut self, results: Vec<ToolResult>) -> anyhow::Result<Vec<RawCall>> {
        // Answer the previous turn's tool calls before asking for the next one.
        if !results.is_empty() {
            let blocks: Vec<Value> = results
                .into_iter()
                .map(|r| {
                    json!({
                        "type": "tool_result",
                        "tool_use_id": r.id,
                        "content": r.content,
                        "is_error": r.is_error,
                    })
                })
                .collect();
            self.messages
                .push(json!({ "role": "user", "content": blocks }));
        }

        let body = post(&self.client, &self.key, &self.request())?;
        let content = body
            .get("content")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Anthropic response missing content: {body}"))?;
        // Echo the assistant turn back so the next request carries the tool_use
        // blocks the tool_results will refer to.
        self.messages
            .push(json!({ "role": "assistant", "content": content }));
        Ok(calls_from_content(&self.messages.last().unwrap()["content"]))
    }
}

/// Pull the `tool_use` blocks out of an assistant content array.
fn calls_from_content(content: &Value) -> Vec<RawCall> {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
        .filter_map(|b| {
            Some(RawCall {
                id: b.get("id")?.as_str()?.to_string(),
                name: b.get("name")?.as_str()?.to_string(),
                args: b.get("input").cloned().unwrap_or_else(|| json!({})),
            })
        })
        .collect()
}

/// POST a request body to the Messages API, returning the parsed JSON or an
/// error carrying the status and body.
fn post(client: &reqwest::blocking::Client, key: &str, req: &Value) -> anyhow::Result<Value> {
    tracing::debug!("anthropic request: {}", req);
    let response = client
        .post(API_URL)
        .header("x-api-key", key)
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
        anyhow::bail!("Status: {status}. Body: {pretty}");
    }
    Ok(body)
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

    #[test]
    fn agent_request_carries_tools_and_seed() {
        let tools = crate::agent::tools();
        let seed = vec![Message::system("SYS"), user(&["selected", "transform"])];
        let agent = AnthropicAgent::new("k".into(), "claude-opus-4-8".into(), &seed, &tools);
        let req = agent.request();

        assert_eq!(req["system"][0]["text"], "SYS");
        assert_eq!(req["messages"][0]["role"], "user");
        assert_eq!(req["messages"][0]["content"][0]["text"], "selected");
        assert_eq!(req["tool_choice"]["type"], "auto");
        let names: Vec<&str> = req["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["edit", "view", "reset", "finish"]);
    }

    #[test]
    fn parses_tool_use_blocks() {
        let content = json!([
            { "type": "text", "text": "let me fix that" },
            { "type": "tool_use", "id": "tu_1", "name": "edit",
              "input": { "old": "a", "new": "b" } },
            { "type": "tool_use", "id": "tu_2", "name": "finish", "input": {} }
        ]);
        let calls = calls_from_content(&content);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].name, "edit");
        assert_eq!(calls[0].args["old"], "a");
        assert_eq!(calls[1].name, "finish");
    }

    #[test]
    fn no_tool_use_is_no_calls() {
        let content = json!([{ "type": "text", "text": "all done" }]);
        assert!(calls_from_content(&content).is_empty());
    }
}
