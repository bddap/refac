//! OpenAI chat-completions API edit-mode agent.

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;

use crate::agent::{Model, RawCall, Seed, Tool, ToolResult};

const API_URL: &str = "https://api.openai.com/v1/chat/completions";

/// One chat-completions message. `untagged` because the assistant variant is a
/// whole verbatim message object that already carries its own `"role"` — a
/// `tag = "role"` discriminant would emit `role` twice. The constructed
/// variants spell their role out instead.
#[derive(Serialize)]
#[serde(untagged)]
enum Message {
    System {
        role: Role,
        content: String,
    },
    User {
        role: Role,
        content: String,
    },
    Tool {
        role: Role,
        tool_call_id: String,
        content: String,
    },
    /// The assistant turn is echoed back verbatim as the API returned it
    /// (`"role"` included). It stays raw `Value` for byte-fidelity:
    /// re-serializing parsed fields would reorder them and drop ones refac
    /// doesn't model, and the next request's `tool_calls`/`tool_call_id`
    /// handshake depends on it matching.
    Assistant(Value),
}

/// A chat-completions message role. Serializes to its lowercase name.
#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Role {
    System,
    User,
    Tool,
}

/// A tool definition as chat-completions takes it: a function wrapper.
#[derive(Serialize)]
struct ToolDef {
    #[serde(rename = "type")]
    kind: FunctionType,
    function: FunctionDef,
}

/// Serializes to the literal `"function"` so a `ToolDef` can't carry any other
/// `type`.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum FunctionType {
    Function,
}

#[derive(Serialize)]
struct FunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

/// An edit-mode session against the chat-completions API. Implements [`Model`]:
/// each `turn` first threads the previous turn's results back as `role: "tool"`
/// messages, posts the running conversation plus the function tools, and returns
/// the model's `tool_calls`. The assistant message is echoed verbatim so the
/// `tool_call_id`s line up — and every tool call gets a result, which the API
/// requires.
pub struct OpenaiAgent {
    key: String,
    model: String,
    client: reqwest::blocking::Client,
    messages: Vec<Message>,
    tools: Vec<ToolDef>,
}

/// The request body POSTed to chat-completions. Borrows the agent's running
/// state so building it never clones the conversation.
#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    messages: &'a [Message],
    tools: &'a [ToolDef],
    tool_choice: &'static str,
}

impl OpenaiAgent {
    pub fn new(key: String, model: String, seed: &Seed, tools: &[Tool]) -> Self {
        let messages = vec![
            Message::System {
                role: Role::System,
                content: seed.system.to_string(),
            },
            Message::User {
                role: Role::User,
                content: seed.selected.to_string(),
            },
            Message::User {
                role: Role::User,
                content: seed.transform.to_string(),
            },
        ];
        let tools = tools
            .iter()
            .map(|t| ToolDef {
                kind: FunctionType::Function,
                function: FunctionDef {
                    name: t.name.to_string(),
                    description: t.description.to_string(),
                    parameters: serde_json::to_value(&t.input_schema)
                        .expect("tool schema serializes"),
                },
            })
            .collect();
        OpenaiAgent {
            key,
            model,
            client: crate::agent::http_client(),
            messages,
            tools,
        }
    }

    fn request(&self) -> Request<'_> {
        Request {
            model: &self.model,
            messages: &self.messages,
            tools: &self.tools,
            tool_choice: "auto",
        }
    }
}

impl Model for OpenaiAgent {
    fn turn(&mut self, results: Vec<ToolResult>) -> anyhow::Result<Vec<RawCall>> {
        // Answer the previous turn's tool calls first. chat-completions has no
        // error flag on a tool message, so mark failures in the content.
        for r in results {
            let content = if r.is_error {
                format!("ERROR: {}", r.content)
            } else {
                r.content
            };
            self.messages.push(Message::Tool {
                role: Role::Tool,
                tool_call_id: r.id,
                content,
            });
        }

        let body = post(&self.client, &self.key, &self.request())?;
        let message = body["choices"][0]["message"].clone();
        if message.is_null() {
            anyhow::bail!("OpenAI response missing a message: {body}");
        }
        let calls = calls_from_message(&message);
        self.messages.push(Message::Assistant(message));
        Ok(calls)
    }
}

/// Pull `tool_calls` out of an assistant message; each `arguments` is a JSON
/// string to parse.
fn calls_from_message(message: &Value) -> Vec<RawCall> {
    message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let function = c.get("function")?;
            let args = function
                .get("arguments")
                .and_then(Value::as_str)
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            Some(RawCall {
                id: c.get("id")?.as_str()?.to_string(),
                name: function.get("name")?.as_str()?.to_string(),
                args,
            })
        })
        .collect()
}

fn post(client: &reqwest::blocking::Client, key: &str, req: &Request) -> anyhow::Result<Value> {
    let response = client
        .post(API_URL)
        .bearer_auth(key)
        .json(req)
        .send()
        .context("Failed to send request to OpenAI API")?;
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
    use serde_json::json;

    /// The wire JSON refac actually sends — the unit tests assert against this,
    /// so they prove the typed structs serialize to the same bytes as before.
    fn request_json(agent: &OpenaiAgent) -> Value {
        serde_json::to_value(agent.request()).unwrap()
    }

    #[test]
    fn agent_request_uses_function_tools() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let agent = OpenaiAgent::new("k".into(), "gpt-5.5".into(), &seed, &tools);
        let req = request_json(&agent);

        assert_eq!(req["tool_choice"], "auto");
        assert_eq!(req["messages"][0]["role"], "system");
        assert_eq!(req["messages"][0]["content"], "SYS");
        assert_eq!(req["messages"][1]["role"], "user");
        assert_eq!(req["messages"][1]["content"], "selected");
        assert_eq!(req["messages"][2]["content"], "transform");
        assert_eq!(req["tools"][0]["type"], "function");
        let names: Vec<&str> = req["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
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
        let mut agent = OpenaiAgent::new("k".into(), "m".into(), &seed, &tools);
        agent.messages.push(Message::Tool {
            role: Role::Tool,
            tool_call_id: "c1".into(),
            content: "ok".into(),
        });
        let req = request_json(&agent);
        let msg = &req["messages"][3];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "c1");
        assert_eq!(msg["content"], "ok");
    }

    #[test]
    fn echoed_assistant_turn_is_verbatim() {
        let tools = crate::agent::tools();
        let seed = Seed {
            system: "SYS",
            selected: "selected",
            transform: "transform",
        };
        let mut agent = OpenaiAgent::new("k".into(), "m".into(), &seed, &tools);
        // The whole assistant message (role included) round-trips unchanged —
        // refac flattens it back in verbatim.
        let raw = json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [
                { "id": "c1", "type": "function",
                  "function": { "name": "edit", "arguments": "{}" } }
            ]
        });
        agent.messages.push(Message::Assistant(raw.clone()));
        assert_eq!(request_json(&agent)["messages"][3], raw);
        // The echoed object already carries `role`; the enum must not add a
        // second one (untagged, not tag = "role").
        let wire = serde_json::to_string(&agent.request()).unwrap();
        assert_eq!(wire.matches("\"role\":\"assistant\"").count(), 1);
    }

    #[test]
    fn parses_tool_calls_with_string_arguments() {
        let message = json!({
            "role": "assistant",
            "tool_calls": [
                { "id": "c1", "type": "function",
                  "function": { "name": "edit", "arguments": "{\"old\":\"a\",\"new\":\"b\"}" } },
                { "id": "c2", "type": "function",
                  "function": { "name": "finish", "arguments": "{}" } }
            ]
        });
        let calls = calls_from_message(&message);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(calls[0].name, "edit");
        assert_eq!(calls[0].args["new"], "b");
        assert_eq!(calls[1].name, "finish");
    }

    #[test]
    fn no_tool_calls_is_no_calls() {
        let message = json!({ "role": "assistant", "content": "done" });
        assert!(calls_from_message(&message).is_empty());
    }
}
