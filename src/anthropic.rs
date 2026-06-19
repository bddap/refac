//! Anthropic (Claude) Messages API edit-mode agent.

use anyhow::Context;
use serde::Serialize;
use serde_json::{json, Value};

use crate::agent::{Model, RawCall, Seed, Tool, ToolResult};

const MAX_TOKENS: u32 = 80000;

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

/// A `system` prompt block. The API only takes text blocks here.
#[derive(Serialize)]
struct SystemBlock {
    #[serde(rename = "type")]
    kind: TextType,
    text: String,
}

/// Serializes to the literal `"text"` so a `SystemBlock`/`ContentBlock::Text`
/// can't carry any other `type`.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum TextType {
    Text,
}

/// One block in a message's `content` array. Tagged by `type` as the Messages
/// API expects: `text`, `tool_use`, `tool_result`.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// One conversation turn. The role tags the JSON (`"role": "user"` /
/// `"assistant"`), so a role can't be paired with the wrong content.
#[derive(Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
enum Message {
    User {
        content: Vec<ContentBlock>,
    },
    /// The assistant turn is echoed back verbatim as the API returned it. It
    /// stays raw `Value` for byte-fidelity: re-serializing parsed blocks would
    /// reorder fields and drop ones refac doesn't model (e.g. `thinking`
    /// signatures), which the next request's `tool_use`/`tool_result` handshake
    /// depends on.
    Assistant {
        content: Value,
    },
}

/// A tool definition as the Messages API takes it.
#[derive(Serialize)]
struct ToolDef {
    name: String,
    description: String,
    input_schema: Value,
}

/// An edit-mode session against the Messages API. Implements [`Model`]: each
/// `turn` first threads the previous turn's results back as a `tool_result` user
/// turn, posts the running conversation plus the tool definitions, and returns
/// the model's tool calls. The assistant's content is echoed back verbatim,
/// which is what the API requires for a `tool_use`/`tool_result` exchange.
pub struct AnthropicAgent {
    key: String,
    model: String,
    client: reqwest::blocking::Client,
    system: Vec<SystemBlock>,
    messages: Vec<Message>,
    tools: Vec<ToolDef>,
}

/// The request body POSTed to the Messages API. Borrows the agent's running
/// state so building it never clones the conversation.
#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: &'a [Message],
    tools: &'a [ToolDef],
    tool_choice: Value,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    system: &'a [SystemBlock],
}

impl AnthropicAgent {
    /// Seed from refac's edit conversation and the tools to expose. The system
    /// prompt goes in the top-level `system`; the user turn carries the selected
    /// text and the instruction as two text blocks.
    pub fn new(key: String, model: String, seed: &Seed, tools: &[Tool]) -> Self {
        let system = vec![SystemBlock {
            kind: TextType::Text,
            text: seed.system.to_string(),
        }];
        let messages = vec![Message::User {
            content: vec![
                ContentBlock::Text {
                    text: field_or_placeholder(seed.selected).to_string(),
                },
                ContentBlock::Text {
                    text: field_or_placeholder(seed.transform).to_string(),
                },
            ],
        }];
        let tools = tools
            .iter()
            .map(|t| ToolDef {
                name: t.name.to_string(),
                description: t.description.to_string(),
                input_schema: serde_json::to_value(&t.input_schema)
                    .expect("tool schema serializes"),
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

    fn request(&self) -> Request<'_> {
        Request {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            messages: &self.messages,
            tools: &self.tools,
            tool_choice: json!({ "type": "auto" }),
            system: &self.system,
        }
    }
}

impl Model for AnthropicAgent {
    fn turn(&mut self, results: Vec<ToolResult>) -> anyhow::Result<Vec<RawCall>> {
        // Answer the previous turn's tool calls before asking for the next one.
        if !results.is_empty() {
            let content = results
                .into_iter()
                .map(|r| ContentBlock::ToolResult {
                    tool_use_id: r.id,
                    content: r.content,
                    is_error: r.is_error,
                })
                .collect();
            self.messages.push(Message::User { content });
        }

        let body = post(&self.client, &self.key, &self.request())?;
        let content = body
            .get("content")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Anthropic response missing content: {body}"))?;
        let calls = calls_from_content(&content);
        // Echo the assistant turn back so the next request carries the tool_use
        // blocks the tool_results will refer to.
        self.messages.push(Message::Assistant { content });
        Ok(calls)
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
fn post(client: &reqwest::blocking::Client, key: &str, req: &Request) -> anyhow::Result<Value> {
    tracing::debug!(
        "anthropic request: {}",
        serde_json::to_value(req).unwrap_or_default()
    );
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

    /// The wire JSON refac actually sends — the unit tests assert against this,
    /// so they prove the typed structs serialize to the same bytes as before.
    fn request_json(agent: &AnthropicAgent) -> Value {
        serde_json::to_value(agent.request()).unwrap()
    }

    #[test]
    fn agent_request_carries_tools_and_seed() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let agent = AnthropicAgent::new("k".into(), "claude-opus-4-8".into(), &seed, &tools);
        let req = request_json(&agent);

        assert_eq!(req["system"][0]["type"], "text");
        assert_eq!(req["system"][0]["text"], "SYS");
        assert_eq!(req["messages"][0]["role"], "user");
        assert_eq!(req["messages"][0]["content"][0]["type"], "text");
        assert_eq!(req["messages"][0]["content"][0]["text"], "selected");
        assert_eq!(req["messages"][0]["content"][1]["text"], "transform");
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
    fn tool_result_turn_serializes_to_wire_shape() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let mut agent = AnthropicAgent::new("k".into(), "m".into(), &seed, &tools);
        agent.messages.push(Message::User {
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".into(),
                content: "ok".into(),
                is_error: false,
            }],
        });
        let req = request_json(&agent);
        let block = &req["messages"][1]["content"][0];
        assert_eq!(req["messages"][1]["role"], "user");
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "tu_1");
        assert_eq!(block["content"], "ok");
        assert_eq!(block["is_error"], false);
    }

    #[test]
    fn echoed_assistant_turn_is_verbatim() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let mut agent = AnthropicAgent::new("k".into(), "m".into(), &seed, &tools);
        // An assistant turn carrying a block type refac doesn't model must
        // round-trip unchanged.
        let raw = json!([
            { "type": "thinking", "thinking": "hmm", "signature": "sig" },
            { "type": "tool_use", "id": "tu_1", "name": "edit", "input": { "old": "a", "new": "b" } }
        ]);
        agent.messages.push(Message::Assistant {
            content: raw.clone(),
        });
        let req = request_json(&agent);
        assert_eq!(req["messages"][1]["role"], "assistant");
        assert_eq!(req["messages"][1]["content"], raw);
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
